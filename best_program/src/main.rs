use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use chrono::{DateTime, Utc};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;

const KEY: &[u8] = b"isu_pt";
const GET_CMD: &[u8] = b"get";
const SERVER1: &str = "95.163.237.76:5123";
const SERVER2: &str = "95.163.237.76:5124";
const OUTPUT_FILE: &str = "sensor_data.txt";

#[derive(Debug)]
enum SensorData {
    TempPressure {
        timestamp: DateTime<Utc>,
        temperature: f32,
        pressure: i16,
    },
    Accelerometer {
        timestamp: DateTime<Utc>,
        x: i32,
        y: i32,
        z: i32,
    },
}

fn verify_checksum(data: &[u8], checksum: u8) -> bool {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    (sum % 256) as u8 == checksum
}

fn connect_and_auth(server: &str, server_name: &str) -> Result<TcpStream, Box<dyn std::error::Error>> {
    println!("[{}] Attempting to connect to {}...", server_name, server);

    let mut stream = TcpStream::connect(server)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    
    stream.write_all(KEY)?;
    
    let mut buf = [0u8; 15];
    stream.read(&mut buf)?;
    
    println!("[{}] ✓ Connected and authenticated", server_name);

    Ok(stream)
}

fn fetch_server1_data(stream: &mut TcpStream) -> Result<SensorData, Box<dyn std::error::Error>> {
    stream.write_all(GET_CMD)?;
    
    let mut buf = [0u8; 15];
    stream.read_exact(&mut buf)?;
    
    let data = &buf[0..14];
    let checksum = buf[14];
    
    if !verify_checksum(data, checksum) {
        return Err("Checksum mismatch".into());
    }
    
    let timestamp_raw = u64::from_be_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ]);
    
    let temperature = f32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    
    let pressure = i16::from_be_bytes([data[12], data[13]]);
    
    let timestamp = DateTime::from_timestamp_micros(timestamp_raw as i64)
        .ok_or("Invalid timestamp")?;
    
    Ok(SensorData::TempPressure {
        timestamp,
        temperature,
        pressure,
    })
}

fn fetch_server2_data(stream: &mut TcpStream) -> Result<SensorData, Box<dyn std::error::Error>> {
    stream.write_all(GET_CMD)?;
    
    let mut buf = [0u8; 21];
    stream.read_exact(&mut buf)?;
    
    let data = &buf[0..20];
    let checksum = buf[20];
    
    if !verify_checksum(data, checksum) {
        return Err("Checksum mismatch".into());
    }
    
    let timestamp_raw = u64::from_be_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ]);
    
    let x = i32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let y = i32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    let z = i32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    
    let timestamp = DateTime::from_timestamp_micros(timestamp_raw as i64)
        .ok_or("Invalid timestamp")?;
    
    Ok(SensorData::Accelerometer {
        timestamp,
        x,
        y,
        z,
    })
}

fn data_collection_loop(
    stream: &mut TcpStream,
    is_server1: bool,
    server_name: &str,
    writer: &Arc<Mutex<BufWriter<std::fs::File>>>,
    stats: &Arc<Mutex<(u64, u64)>>,
    running: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut packet_count = 0u64;
    let mut error_count = 0u64;
    
    while running.load(Ordering::SeqCst) {
        let result = if is_server1 {
            fetch_server1_data(stream)
        } else {
            fetch_server2_data(stream)
        };
        
        match result {
            Ok(data) => {
                packet_count += 1;
                error_count = 0;
                
                let line = match data {
                    SensorData::TempPressure {
                        timestamp,
                        temperature,
                        pressure,
                    } => {
                        format!(
                            "{} [S1] temperature={:.2}C pressure={}\n",
                            timestamp.format("%Y-%m-%d %H:%M:%S"),
                            temperature,
                            pressure
                        )
                    }
                    SensorData::Accelerometer { timestamp, x, y, z } => {
                        format!(
                            "{} [S2] x={} y={} z={}\n",
                            timestamp.format("%Y-%m-%d %H:%M:%S"),
                            x,
                            y,
                            z
                        )
                    }
                };
                
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(line.as_bytes()) {
                        eprintln!("[{}] ✗ Write error: {}", server_name, e);
                    }
                }
                
                if let Ok(mut s) = stats.lock() {
                    if is_server1 {
                        s.0 = packet_count;
                    } else {
                        s.1 = packet_count;
                    }
                }
                
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                error_count += 1;
                eprintln!("[{}] ✗ Data fetch error ({}): {}", server_name, error_count, e);
                
                if error_count >= 3 {
                    return Err(format!("Too many consecutive errors: {}", error_count).into());
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    
    Ok(())
}

fn worker_thread(
    server: &str,
    is_server1: bool,
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    stats: Arc<Mutex<(u64, u64)>>,
    running: Arc<AtomicBool>,
) {
    let server_name = if is_server1 { "Server1" } else { "Server2" };
    let mut reconnect_delay = Duration::from_secs(1);
    let max_delay = Duration::from_secs(30);
    let mut total_reconnects = 0u64;
    
    println!("[{}] Worker thread started", server_name);

    while running.load(Ordering::SeqCst) {
        match connect_and_auth(server, server_name) {
            Ok(mut stream) => {
                if total_reconnects > 0 {
                    println!("[{}] ✓ Reconnected successfully (attempt #{})", server_name, total_reconnects);
                } else {
                    println!("[{}] Connected successfully", server_name);
                }
                reconnect_delay = Duration::from_secs(1);
                
                match data_collection_loop(&mut stream, is_server1, server_name, &writer, &stats, &running) {
                    Ok(_) => {
                        println!("[{}] Data collection loop ended gracefully", server_name);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[{}] ✗ Connection lost: {}", server_name, e);
                        total_reconnects += 1;
                    }
                }
                
                drop(stream);
            }
            Err(e) => {
                eprintln!("[{}] ✗ Connection failed: {}", server_name, e);
                total_reconnects += 1;
            }
        } 
        
        if running.load(Ordering::SeqCst) {
            println!("[{}] Waiting {:?} before reconnection attempt...", server_name, reconnect_delay);
            
            let mut elapsed = Duration::ZERO;
            while elapsed < reconnect_delay && running.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
                elapsed += Duration::from_millis(100);
            }
            
            reconnect_delay = std::cmp::min(reconnect_delay * 2, max_delay);
        }
    }
    
    println!("[{}] Worker thread finished", server_name);
}

#[cfg(not(test))]
fn main() {
    println!("Starting network data logger...");

    println!("Server 1: {} (Temperature/Pressure)", SERVER1);
    println!("Server 2: {} (Accelerometer)", SERVER2);
    println!("Output file: {}", OUTPUT_FILE);

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        eprintln!("\n[INFO] Ctrl+C received. Attempting graceful shutdown...");
    })
    .expect("Error setting Ctrl-C handler");
    
    println!("Press Ctrl+C to stop");
    
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(OUTPUT_FILE)
        .expect("Failed to open output file");
    
    let writer = Arc::new(Mutex::new(BufWriter::new(file)));
    let stats = Arc::new(Mutex::new((0u64, 0u64)));

    let writer1 = Arc::clone(&writer);
    let stats1 = Arc::clone(&stats);
    let running1 = Arc::clone(&running);
    let handle1 = thread::spawn(move || {
        worker_thread(SERVER1, true, writer1, stats1, running1);
    });
    
    let writer2 = Arc::clone(&writer);
    let stats2 = Arc::clone(&stats);
    let running2 = Arc::clone(&running);
    let handle2 = thread::spawn(move || {
        worker_thread(SERVER2, false, writer2, stats2, running2);
    });
    
    let writer3 = Arc::clone(&writer);
    let stats3 = Arc::clone(&stats);
    let running3 = Arc::clone(&running);
    let handle3 = thread::spawn(move || {
        while running3.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(10));

            if let Ok(mut w) = writer3.lock() {
                if let Err(e) = w.flush() {
                    eprintln!("✗ Flush error: {}", e);
                }
            }
            
            if let Ok(s) = stats3.lock() {
                println!("\n[STATS] Server1: {} packets | Server2: {} packets", s.0, s.1);
            }
        }
        
        if let Ok(mut w) = writer3.lock() {
            if let Err(e) = w.flush() {
                eprintln!("✗ Final Flush error: {}", e);
            }
        }
    });
    
    handle1.join().unwrap();
    handle2.join().unwrap();
    handle3.join().unwrap();
    
    println!("\n[INFO] All threads finished. Logger stopped gracefully.");
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[test]
    fn test_verify_checksum_valid() {
        let data = vec![1, 2, 3, 4, 5];
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let checksum = (sum % 256) as u8;
        
        assert!(verify_checksum(&data, checksum));
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let data = vec![1, 2, 3, 4, 5];
        let wrong_checksum = 99u8;
        
        assert!(!verify_checksum(&data, wrong_checksum));
    }

    #[test]
    fn test_verify_checksum_empty() {
        let data = vec![];
        assert!(verify_checksum(&data, 0));
    }

    #[test]
    fn test_verify_checksum_overflow() {
        let data = vec![255, 255, 255];
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let checksum = (sum % 256) as u8;
        
        assert!(verify_checksum(&data, checksum));
    }

    #[test]
    fn test_sensor_data_temp_pressure_format() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data = SensorData::TempPressure {
            timestamp,
            temperature: 25.5,
            pressure: 1013,
        };
        
        match data {
            SensorData::TempPressure { temperature, pressure, .. } => {
                assert_eq!(temperature, 25.5);
                assert_eq!(pressure, 1013);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_sensor_data_accelerometer_format() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data = SensorData::Accelerometer {
            timestamp,
            x: 100,
            y: -200,
            z: 300,
        };
        
        match data {
            SensorData::Accelerometer { x, y, z, .. } => {
                assert_eq!(x, 100);
                assert_eq!(y, -200);
                assert_eq!(z, 300);
            }
            _ => panic!("Wrong variant"),
        }
    }

    fn mock_server(port: u16, response: Vec<u8>, auth_required: bool) {
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            listener.set_nonblocking(false).unwrap();
            
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                if auth_required {
                    let mut auth_buf = vec![0u8; KEY.len()];
                    let _ = stream.read_exact(&mut auth_buf);
                    
                    if auth_buf == KEY {
                        let _ = stream.write_all(b"AUTH_OK_123456\n");
                    }
                }
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() && cmd_buf == GET_CMD {
                    let _ = stream.write_all(&response);
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn test_fetch_server1_data_valid() {
        let port = 15123;
        
        let timestamp: u64 = 1000000000;
        let temperature: f32 = 23.5;
        let pressure: i16 = 1013;
        
        let mut data = Vec::new();
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(&temperature.to_be_bytes());
        data.extend_from_slice(&pressure.to_be_bytes());
        
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let checksum = (sum % 256) as u8;
        data.push(checksum);
        
        mock_server(port, data, true);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let result = fetch_server1_data(&mut stream);
        assert!(result.is_ok());
        
        if let Ok(SensorData::TempPressure { temperature: t, pressure: p, .. }) = result {
            assert_eq!(t, 23.5);
            assert_eq!(p, 1013);
        }
    }

    #[test]
    fn test_fetch_server1_data_checksum_error() {
        let port = 15124;
        
        let mut data = vec![0u8; 14];
        data.push(255);
        
        mock_server(port, data, true);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let result = fetch_server1_data(&mut stream);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Checksum"));
    }

    #[test]
    fn test_fetch_server2_data_valid() {
        let port = 15125;
        
        let timestamp: u64 = 2000000000;
        let x: i32 = 100;
        let y: i32 = -200;
        let z: i32 = 300;
        
        let mut data = Vec::new();
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(&x.to_be_bytes());
        data.extend_from_slice(&y.to_be_bytes());
        data.extend_from_slice(&z.to_be_bytes());
        
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let checksum = (sum % 256) as u8;
        data.push(checksum);
        
        mock_server(port, data, true);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let result = fetch_server2_data(&mut stream);
        assert!(result.is_ok());
        
        if let Ok(SensorData::Accelerometer { x: ax, y: ay, z: az, .. }) = result {
            assert_eq!(ax, 100);
            assert_eq!(ay, -200);
            assert_eq!(az, 300);
        }
    }

    #[test]
    fn test_fetch_server2_data_checksum_error() {
        let port = 15126;
        
        let mut data = vec![0u8; 20];
        data.push(123);
        
        mock_server(port, data, true);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let result = fetch_server2_data(&mut stream);
        assert!(result.is_err());
    }

    #[test]
    fn test_connect_and_auth_success() {
        let port = 15127;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut buf);
                if buf == KEY {
                    let _ = stream.write_all(b"AUTH_OK_123456\n");
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let result = connect_and_auth(&format!("127.0.0.1:{}", port), "TestServer");
        assert!(result.is_ok());
    }

    #[test]
    fn test_connect_and_auth_connection_refused() {
        let result = connect_and_auth("127.0.0.1:19999", "TestServer");
        assert!(result.is_err());
    }

    #[test]
    fn test_sensor_data_debug() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data1 = SensorData::TempPressure {
            timestamp,
            temperature: 25.5,
            pressure: 1013,
        };
        
        let debug_str = format!("{:?}", data1);
        assert!(debug_str.contains("TempPressure"));
        assert!(debug_str.contains("25.5"));
        assert!(debug_str.contains("1013"));
        
        let data2 = SensorData::Accelerometer {
            timestamp,
            x: 1,
            y: 2,
            z: 3,
        };
        
        let debug_str2 = format!("{:?}", data2);
        assert!(debug_str2.contains("Accelerometer"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(KEY, b"isu_pt");
        assert_eq!(GET_CMD, b"get");
        assert_eq!(SERVER1, "95.163.237.76:5123");
        assert_eq!(SERVER2, "95.163.237.76:5124");
        assert_eq!(OUTPUT_FILE, "sensor_data.txt");
    }

    #[test]
    fn test_atomic_bool_operations() {
        let running = AtomicBool::new(true);
        assert!(running.load(Ordering::SeqCst));
        
        running.store(false, Ordering::SeqCst);
        assert!(!running.load(Ordering::SeqCst));
    }

    #[test]
    fn test_arc_mutex_stats() {
        let stats = Arc::new(Mutex::new((0u64, 0u64)));
        
        {
            let mut s = stats.lock().unwrap();
            s.0 = 100;
            s.1 = 200;
        }
        
        let s = stats.lock().unwrap();
        assert_eq!(s.0, 100);
        assert_eq!(s.1, 200);
    }

    #[test]
    fn test_invalid_timestamp() {
        // Тест с невалидным timestamp
        let invalid_timestamp = i64::MAX;
        let result = DateTime::from_timestamp_micros(invalid_timestamp);
        assert!(result.is_none());
    }

    #[test]
    fn test_duration_operations() {
        let d1 = Duration::from_secs(1);
        let d2 = Duration::from_secs(2);
        let d3 = d1 + d2;
        
        assert_eq!(d3, Duration::from_secs(3));
        
        let d4 = std::cmp::min(Duration::from_secs(10), Duration::from_secs(30));
        assert_eq!(d4, Duration::from_secs(10));
    }

    #[test]
    fn test_data_collection_loop_server1_success() {
        let port = 16001;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(Mutex::new((0u64, 0u64)));
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK_123456\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let timestamp: u64 = 1700000000000;
                    let temperature: f32 = 22.5;
                    let pressure: i16 = 1010;
                    
                    let mut data = Vec::new();
                    data.extend_from_slice(&timestamp.to_be_bytes());
                    data.extend_from_slice(&temperature.to_be_bytes());
                    data.extend_from_slice(&pressure.to_be_bytes());
                    
                    let sum: u32 = data.iter().map(|&b| b as u32).sum();
                    let checksum = (sum % 256) as u8;
                    data.push(checksum);
                    
                    let _ = stream.write_all(&data);
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        let result = data_collection_loop(
            &mut stream,
            true,
            "TestServer1",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_ok());
        
        let s = stats.lock().unwrap();
        assert!(s.0 > 0);
    }

    #[test]
    fn test_data_collection_loop_consecutive_errors() {
        let port = 16002;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(Mutex::new((0u64, 0u64)));
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK_123456\n");
                
                for _ in 0..3 {
                    let mut cmd_buf = vec![0u8; GET_CMD.len()];
                    if stream.read_exact(&mut cmd_buf).is_ok() {
                        let bad_data = vec![0u8; 14];
                        let wrong_checksum = 255u8;
                        let mut response = bad_data;
                        response.push(wrong_checksum);
                        let _ = stream.write_all(&response);
                    }
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let result = data_collection_loop(
            &mut stream,
            true,
            "TestServer1",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many consecutive errors"));
    }

    #[test]
    fn test_data_collection_loop_server2_success() {
        let port = 16003;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(Mutex::new((0u64, 0u64)));
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK_123456\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let timestamp: u64 = 1800000000000;
                    let x: i32 = 150;
                    let y: i32 = -250;
                    let z: i32 = 350;
                    
                    let mut data = Vec::new();
                    data.extend_from_slice(&timestamp.to_be_bytes());
                    data.extend_from_slice(&x.to_be_bytes());
                    data.extend_from_slice(&y.to_be_bytes());
                    data.extend_from_slice(&z.to_be_bytes());
                    
                    let sum: u32 = data.iter().map(|&b| b as u32).sum();
                    let checksum = (sum % 256) as u8;
                    data.push(checksum);
                    
                    let _ = stream.write_all(&data);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 15];
        stream.read(&mut auth_buf).unwrap();
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        let result = data_collection_loop(
            &mut stream,
            false,
            "TestServer2",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_ok());
        
        let s = stats.lock().unwrap();
        assert!(s.1 > 0);
    }

    #[test]
    fn test_worker_thread_reconnection() {
        let port = 16004;
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(Mutex::new((0u64, 0u64)));
        let running = Arc::new(AtomicBool::new(true));
        
        let running_clone = Arc::clone(&running);
        
        thread::spawn(move || {
            for attempt in 0..2 {
                let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
                listener.set_nonblocking(false).unwrap();
                
                if let Ok((mut stream, _)) = listener.accept() {
                    stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                    
                    let mut auth_buf = vec![0u8; KEY.len()];
                    if stream.read_exact(&mut auth_buf).is_ok() {
                        let _ = stream.write_all(b"AUTH_OK_123456\n");
                    }
                    
                    if attempt == 0 {
                        drop(stream);
                    } else {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let running_stopper = Arc::clone(&running_clone);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            running_stopper.store(false, Ordering::SeqCst);
        });
        
        worker_thread(
            &format!("127.0.0.1:{}", port),
            true,
            writer,
            stats,
            running_clone,
        );
    }

    #[test]
    fn test_sensor_data_formatting() {
        let timestamp = DateTime::from_timestamp_micros(1700000000000000).unwrap();
        
        let temp_data = SensorData::TempPressure {
            timestamp,
            temperature: 25.75,
            pressure: 1015,
        };
        
        let formatted = match temp_data {
            SensorData::TempPressure {
                timestamp,
                temperature,
                pressure,
            } => {
                format!(
                    "{} [S1] temperature={:.2}C pressure={}\n",
                    timestamp.format("%Y-%m-%d %H:%M:%S"),
                    temperature,
                    pressure
                )
            }
            _ => String::new(),
        };
        
        assert!(formatted.contains("[S1]"));
        assert!(formatted.contains("temperature=25.75C"));
        assert!(formatted.contains("pressure=1015"));
        
        let accel_data = SensorData::Accelerometer {
            timestamp,
            x: 100,
            y: -200,
            z: 300,
        };
        
        let formatted2 = match accel_data {
            SensorData::Accelerometer { timestamp, x, y, z } => {
                format!(
                    "{} [S2] x={} y={} z={}\n",
                    timestamp.format("%Y-%m-%d %H:%M:%S"),
                    x,
                    y,
                    z
                )
            }
            _ => String::new(),
        };
        
        assert!(formatted2.contains("[S2]"));
        assert!(formatted2.contains("x=100"));
        assert!(formatted2.contains("y=-200"));
        assert!(formatted2.contains("z=300"));
    }

    #[test]
    fn test_large_values() {
        let data = vec![255u8; 100];
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        let checksum = (sum % 256) as u8;
        
        assert!(verify_checksum(&data, checksum));
    }

    #[test]
    fn test_timestamp_edge_cases() {
        let min_timestamp = DateTime::from_timestamp_micros(0);
        assert!(min_timestamp.is_some());
        
        let future = DateTime::from_timestamp_micros(2000000000000000);
        assert!(future.is_some());
    }

    #[test]
    fn test_file_writing() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        
        let test_data = "Test line\n";
        
        {
            let mut w = writer.lock().unwrap();
            w.write_all(test_data.as_bytes()).unwrap();
            w.flush().unwrap();
        }
        
        let metadata = temp_file.as_file().metadata().unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_temperature_range() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        
        let data1 = SensorData::TempPressure {
            timestamp,
            temperature: -40.0,
            pressure: 1000,
        };
        
        if let SensorData::TempPressure { temperature, .. } = data1 {
            assert_eq!(temperature, -40.0);
        }
        
        let data2 = SensorData::TempPressure {
            timestamp,
            temperature: 150.0,
            pressure: 2000,
        };
        
        if let SensorData::TempPressure { temperature, .. } = data2 {
            assert_eq!(temperature, 150.0);
        }
    }

    #[test]
    fn test_accelerometer_range() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        
        let data1 = SensorData::Accelerometer {
            timestamp,
            x: i32::MAX,
            y: i32::MIN,
            z: 0,
        };
        
        if let SensorData::Accelerometer { x, y, z, .. } = data1 {
            assert_eq!(x, i32::MAX);
            assert_eq!(y, i32::MIN);
            assert_eq!(z, 0);
        }
    }
}