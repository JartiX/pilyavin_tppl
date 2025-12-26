use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use chrono::{DateTime, Utc};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use ctrlc;
use socket2::{Socket, Domain, Type, Protocol};
use std::net::SocketAddr;

const KEY: &[u8] = b"isu_pt";
const GET_CMD: &[u8] = b"get";
const SERVER1: &str = "95.163.237.76:5123";
const SERVER2: &str = "95.163.237.76:5124";
const OUTPUT_FILE: &str = "sensor_data.txt";

const SERVER1_PACKET_SIZE: usize = 15; // 8 + 4 + 2 + 1
const SERVER2_PACKET_SIZE: usize = 21; // 8 + 4 + 4 + 4 + 1

const READ_TIMEOUT_MS: u64 = 4500;
const WRITE_TIMEOUT_MS: u64 = 2000;
const MAX_CONSECUTIVE_ERRORS: u32 = 3;    
const REQUEST_DELAY_MS: u64 = 1;
const MIN_RECONNECT_DELAY_MS: u64 = 20;
#[allow(dead_code)]
const MAX_RECONNECT_DELAY_MS: u64 = 1000;
const STATS_INTERVAL_SECS: u64 = 10;
const FLUSH_INTERVAL_SECS: u64 = 5;

#[derive(Debug, Clone)]
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

#[derive(Debug, Default)]
struct ServerStats {
    packets_received: AtomicU64,
    checksum_errors: AtomicU64,
    timeout_errors: AtomicU64,
    connection_errors: AtomicU64,
    reconnections: AtomicU64,
    sync_resets: AtomicU64,
}

impl ServerStats {
    fn new() -> Self {
        Self::default()
    }
}

fn calculate_checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    (sum % 256) as u8
}

#[allow(dead_code)]
fn verify_checksum(data: &[u8], checksum: u8) -> bool {
    calculate_checksum(data) == checksum
}

/// Создание TCP соединения с оптимальными настройками
fn create_optimized_socket(addr: &str) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
    let socket_addr: SocketAddr = addr.parse()?;
    
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    
    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_recv_buffer_size(65536)?;
    socket.set_send_buffer_size(65536)?;
    socket.set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)))?;
    socket.set_write_timeout(Some(Duration::from_millis(WRITE_TIMEOUT_MS)))?;
    
    socket.connect_timeout(&socket_addr.into(), Duration::from_secs(5))?;
    
    Ok(socket.into())
}

#[allow(dead_code)]
fn drain_input_buffer(stream: &mut TcpStream) -> usize {
    let old_timeout = stream.read_timeout().ok().flatten();
    let _ = stream.set_read_timeout(Some(Duration::from_millis(30)));
    
    let mut total_drained = 0;
    let mut buf = [0u8; 512];
    let mut attempts = 0;
    
    while attempts < 3 {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                total_drained += n;
                attempts = 0;
                if total_drained > 2048 {
                    break;
                }
            }
            Err(_) => {
                attempts += 1;
                if attempts < 3 {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    
    let _ = stream.set_read_timeout(old_timeout);
    
    total_drained
}

fn connect_and_auth(
    server: &str, 
    server_name: &str,
    stats: &ServerStats,
) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = create_optimized_socket(server)?;
    
    stream.write_all(KEY)?;
    stream.flush()?;
    
    let mut auth_buf = [0u8; 64];
    let mut total = 0;
    let start = Instant::now();
    
    while start.elapsed() < Duration::from_secs(3) {
        match stream.read(&mut auth_buf[total..]) {
            Ok(0) => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(n) => {
                total += n;
                thread::sleep(Duration::from_millis(20));
                let _ = stream.set_read_timeout(Some(Duration::from_millis(30)));
                match stream.read(&mut auth_buf[total..]) {
                    Ok(n2) if n2 > 0 => total += n2,
                    _ => {}
                }
                let _ = stream.set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                stats.connection_errors.fetch_add(1, Ordering::Relaxed);
                return Err(e.into());
            }
        }
    }
    
    if total == 0 {
        stats.connection_errors.fetch_add(1, Ordering::Relaxed);
        return Err("No auth response received".into());
    }
    
    println!("[{}] ✓ Connected ({} bytes)", server_name, total);
    
    Ok(stream)
}

fn read_exact_reliable(
    stream: &mut TcpStream, 
    buf: &mut [u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut total_read = 0;
    let target_len = buf.len();
    let start = Instant::now();
    let timeout = Duration::from_millis(READ_TIMEOUT_MS);
    
    while total_read < target_len {
        if start.elapsed() > timeout {
            return Err(format!("Read timeout: got {}/{} bytes", total_read, target_len).into());
        }
        
        match stream.read(&mut buf[total_read..]) {
            Ok(0) => {
                return Err("Connection closed by server".into());
            }
            Ok(n) => {
                total_read += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock 
                   || e.kind() == std::io::ErrorKind::TimedOut => {
                if start.elapsed() > timeout {
                    return Err(format!("Read timeout: got {}/{} bytes", total_read, target_len).into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
    
    Ok(())
}

fn fetch_server1_data(
    stream: &mut TcpStream,
    stats: &ServerStats,
) -> Result<SensorData, Box<dyn std::error::Error + Send + Sync>> {
    stream.write_all(GET_CMD)?;
    stream.flush()?;
    
    let mut buf = [0u8; SERVER1_PACKET_SIZE];
    read_exact_reliable(stream, &mut buf)?;
    
    let data = &buf[0..14];
    let checksum = buf[14];
    let calculated = calculate_checksum(data);
    
    if calculated != checksum {
        stats.checksum_errors.fetch_add(1, Ordering::Relaxed);
        return Err(format!("Checksum mismatch: calculated {}, received {}", 
                          calculated, checksum).into());
    }
    
    let timestamp_raw = u64::from_be_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ]);
    
    let temperature = f32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let pressure = i16::from_be_bytes([data[12], data[13]]);
    
    let timestamp = DateTime::from_timestamp_micros(timestamp_raw as i64)
        .ok_or("Invalid timestamp")?;
    
    stats.packets_received.fetch_add(1, Ordering::Relaxed);
    
    Ok(SensorData::TempPressure {
        timestamp,
        temperature,
        pressure,
    })
}

fn fetch_server2_data(
    stream: &mut TcpStream,
    stats: &ServerStats,
) -> Result<SensorData, Box<dyn std::error::Error + Send + Sync>> {
    stream.write_all(GET_CMD)?;
    stream.flush()?;
    
    let mut buf = [0u8; SERVER2_PACKET_SIZE];
    read_exact_reliable(stream, &mut buf)?;
    
    let data = &buf[0..20];
    let checksum = buf[20];
    let calculated = calculate_checksum(data);
    
    if calculated != checksum {
        stats.checksum_errors.fetch_add(1, Ordering::Relaxed);
        return Err(format!("Checksum mismatch: calculated {}, received {}", 
                          calculated, checksum).into());
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
    
    stats.packets_received.fetch_add(1, Ordering::Relaxed);
    
    Ok(SensorData::Accelerometer {
        timestamp,
        x,
        y,
        z,
    })
}

fn format_data(data: &SensorData) -> String {
    match data {
        SensorData::TempPressure { timestamp, temperature, pressure } => {
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
                x, y, z
            )
        }
    }
}

fn data_collection_loop(
    stream: &mut TcpStream,
    is_server1: bool,
    server_name: &str,
    writer: &Arc<Mutex<BufWriter<std::fs::File>>>,
    stats: &Arc<ServerStats>,
    running: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut consecutive_errors = 0u32;
    let mut last_success = Instant::now();
    
    while running.load(Ordering::SeqCst) {
        let result = if is_server1 {
            fetch_server1_data(stream, stats)
        } else {
            fetch_server2_data(stream, stats)
        };
        
        match result {
            Ok(data) => {
                consecutive_errors = 0;
                last_success = Instant::now();
                
                let line = format_data(&data);
                
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(line.as_bytes()) {
                        eprintln!("[{}] ✗ Write error: {}", server_name, e);
                    }
                }
                
                thread::sleep(Duration::from_millis(REQUEST_DELAY_MS));
            }
            Err(e) => {
                consecutive_errors += 1;
                let error_msg = e.to_string();
                
                if error_msg.contains("Checksum") {
                    stats.sync_resets.fetch_add(1, Ordering::Relaxed);
                    return Err("Stream desync".into());
                }
                
                if error_msg.contains("timeout") || error_msg.contains("10060") {
                    stats.timeout_errors.fetch_add(1, Ordering::Relaxed);
                }
                
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    return Err(format!("Too many errors: {}", consecutive_errors).into());
                }
            }
        }
        
        if last_success.elapsed() > Duration::from_secs(5) {
            return Err("Stalled".into());
        }
    }
    
    Ok(())
}

fn worker_thread(
    server: &str,
    is_server1: bool,
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    stats: Arc<ServerStats>,
    running: Arc<AtomicBool>,
) {
    let server_name = if is_server1 { "Server1" } else { "Server2" };
    
    println!("[{}] Worker started", server_name);

    while running.load(Ordering::SeqCst) {
        match connect_and_auth(server, server_name, &stats) {
            Ok(mut stream) => {
                let reconnects = stats.reconnections.load(Ordering::Relaxed);
                if reconnects > 0 {
                    println!("[{}] ✓ Reconnected (#{})", server_name, reconnects);
                }
                
                match data_collection_loop(&mut stream, is_server1, server_name, &writer, &stats, &running) {
                    Ok(_) => {
                        println!("[{}] Loop ended gracefully", server_name);
                        break;
                    }
                    Err(_) => {
                        stats.reconnections.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(e) => {
                eprintln!("[{}] ✗ Connect failed: {}", server_name, e);
                stats.connection_errors.fetch_add(1, Ordering::Relaxed);
                stats.reconnections.fetch_add(1, Ordering::Relaxed);
            }
        } 
        
        if running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(MIN_RECONNECT_DELAY_MS));
        }
    }
    
    println!("[{}] Worker finished", server_name);
}

fn stats_and_flush_thread(
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    stats1: Arc<ServerStats>,
    stats2: Arc<ServerStats>,
    running: Arc<AtomicBool>,
) {
    let mut last_flush = Instant::now();
    let mut last_stats = Instant::now();
    
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));
        
        if last_flush.elapsed() >= Duration::from_secs(FLUSH_INTERVAL_SECS) {
            if let Ok(mut w) = writer.lock() {
                let _ = w.flush();
            }
            last_flush = Instant::now();
        }
        
        if last_stats.elapsed() >= Duration::from_secs(STATS_INTERVAL_SECS) {
            let p1 = stats1.packets_received.load(Ordering::Relaxed);
            let p2 = stats2.packets_received.load(Ordering::Relaxed);
            let c1 = stats1.checksum_errors.load(Ordering::Relaxed);
            let c2 = stats2.checksum_errors.load(Ordering::Relaxed);
            let r1 = stats1.reconnections.load(Ordering::Relaxed);
            let r2 = stats2.reconnections.load(Ordering::Relaxed);
            let s1 = stats1.sync_resets.load(Ordering::Relaxed);
            let s2 = stats2.sync_resets.load(Ordering::Relaxed);
            
            println!("\n[STATS] S1: {} ok, {} csum_err, {} reconn, {} sync | S2: {} ok, {} csum_err, {} reconn, {} sync",
                     p1, c1, r1, s1, p2, c2, r2, s2);
            
            last_stats = Instant::now();
        }
    }
    
    if let Ok(mut w) = writer.lock() {
        let _ = w.flush();
    }
}

#[cfg(not(test))]
fn main() {
    println!("Server 1: {}", SERVER1);
    println!("Server 2: {}", SERVER2);
    println!("Output: {}", OUTPUT_FILE);

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
        eprintln!("\n[INFO] Ctrl+C received. Shutting down...");
    })
    .expect("Error setting Ctrl-C handler");
    
    println!("Press Ctrl+C to stop\n");
    
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(OUTPUT_FILE)
        .expect("Failed to open output file");
    
    let writer = Arc::new(Mutex::new(BufWriter::with_capacity(65536, file)));
    
    let stats1 = Arc::new(ServerStats::new());
    let stats2 = Arc::new(ServerStats::new());

    let writer1 = Arc::clone(&writer);
    let stats1_clone = Arc::clone(&stats1);
    let running1 = Arc::clone(&running);
    let handle1 = thread::spawn(move || {
        worker_thread(SERVER1, true, writer1, stats1_clone, running1);
    });
    
    let writer2 = Arc::clone(&writer);
    let stats2_clone = Arc::clone(&stats2);
    let running2 = Arc::clone(&running);
    let handle2 = thread::spawn(move || {
        worker_thread(SERVER2, false, writer2, stats2_clone, running2);
    });
    
    let writer3 = Arc::clone(&writer);
    let stats1_for_stats = Arc::clone(&stats1);
    let stats2_for_stats = Arc::clone(&stats2);
    let running3 = Arc::clone(&running);
    let handle3 = thread::spawn(move || {
        stats_and_flush_thread(writer3, stats1_for_stats, stats2_for_stats, running3);
    });
    
    handle1.join().unwrap();
    handle2.join().unwrap();
    handle3.join().unwrap();
    
    println!("                 FINAL STATISTICS               ");
    println!("Server 1:");
    println!("   Packets: {:>10}", stats1.packets_received.load(Ordering::Relaxed));
    println!("   Checksum errors: {:>10}", stats1.checksum_errors.load(Ordering::Relaxed));
    println!("   Sync resets: {:>10}", stats1.sync_resets.load(Ordering::Relaxed));
    println!("   Reconnections: {:>10}", stats1.reconnections.load(Ordering::Relaxed));
    println!(" Server 2:");
    println!("   Packets: {:>10}", stats2.packets_received.load(Ordering::Relaxed));
    println!("   Checksum errors: {:>10}", stats2.checksum_errors.load(Ordering::Relaxed));
    println!("   Sync resets: {:>10}", stats2.sync_resets.load(Ordering::Relaxed));
    println!("   Reconnections: {:>10}", stats2.reconnections.load(Ordering::Relaxed));

    let total = stats1.packets_received.load(Ordering::Relaxed)
              + stats2.packets_received.load(Ordering::Relaxed);
    println!("\n[INFO] Total packets collected: {}", total);
    println!("[INFO] Logger stopped gracefully.");
}

// ==================== TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    // ============ CHECKSUM TESTS ============
    
    #[test]
    fn test_calculate_checksum() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(calculate_checksum(&data), 15);
        
        let data2 = vec![255, 255];
        assert_eq!(calculate_checksum(&data2), (510 % 256) as u8);
    }

    #[test]
    fn test_calculate_checksum_empty() {
        let data: Vec<u8> = vec![];
        assert_eq!(calculate_checksum(&data), 0);
    }

    #[test]
    fn test_calculate_checksum_large() {
        let data = vec![255u8; 1000];
        let expected = ((255u32 * 1000) % 256) as u8;
        assert_eq!(calculate_checksum(&data), expected);
    }

    #[test]
    fn test_verify_checksum_valid() {
        let data = vec![1, 2, 3, 4, 5];
        let checksum = calculate_checksum(&data);
        assert!(verify_checksum(&data, checksum));
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let data = vec![1, 2, 3, 4, 5];
        assert!(!verify_checksum(&data, 99));
    }

    #[test]
    fn test_verify_checksum_empty() {
        let data: Vec<u8> = vec![];
        assert!(verify_checksum(&data, 0));
    }

    // ============ SENSOR DATA TESTS ============

    #[test]
    fn test_format_data_temp_pressure() {
        let timestamp = DateTime::from_timestamp_micros(1700000000000000).unwrap();
        let data = SensorData::TempPressure {
            timestamp,
            temperature: 25.5,
            pressure: 1013,
        };
        
        let formatted = format_data(&data);
        assert!(formatted.contains("[S1]"));
        assert!(formatted.contains("temperature=25.50C"));
        assert!(formatted.contains("pressure=1013"));
        assert!(formatted.ends_with('\n'));
    }

    #[test]
    fn test_format_data_accelerometer() {
        let timestamp = DateTime::from_timestamp_micros(1700000000000000).unwrap();
        let data = SensorData::Accelerometer {
            timestamp,
            x: 100,
            y: -200,
            z: 300,
        };
        
        let formatted = format_data(&data);
        assert!(formatted.contains("[S2]"));
        assert!(formatted.contains("x=100"));
        assert!(formatted.contains("y=-200"));
        assert!(formatted.contains("z=300"));
        assert!(formatted.ends_with('\n'));
    }

    #[test]
    fn test_sensor_data_clone() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data = SensorData::TempPressure {
            timestamp,
            temperature: 25.5,
            pressure: 1013,
        };
        
        let cloned = data.clone();
        match (data, cloned) {
            (SensorData::TempPressure { temperature: t1, pressure: p1, .. }, 
             SensorData::TempPressure { temperature: t2, pressure: p2, .. }) => {
                assert_eq!(t1, t2);
                assert_eq!(p1, p2);
            }
            _ => panic!("Clone mismatch"),
        }
    }

    #[test]
    fn test_sensor_data_clone_accelerometer() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data = SensorData::Accelerometer {
            timestamp,
            x: 1,
            y: 2,
            z: 3,
        };
        
        let cloned = data.clone();
        match (data, cloned) {
            (SensorData::Accelerometer { x: x1, y: y1, z: z1, .. }, 
             SensorData::Accelerometer { x: x2, y: y2, z: z2, .. }) => {
                assert_eq!(x1, x2);
                assert_eq!(y1, y2);
                assert_eq!(z1, z2);
            }
            _ => panic!("Clone mismatch"),
        }
    }

    #[test]
    fn test_sensor_data_debug() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        let data = SensorData::TempPressure {
            timestamp,
            temperature: 25.5,
            pressure: 1013,
        };
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("TempPressure"));
        
        let data2 = SensorData::Accelerometer {
            timestamp,
            x: 1, y: 2, z: 3,
        };
        let debug_str2 = format!("{:?}", data2);
        assert!(debug_str2.contains("Accelerometer"));
    }

    // ============ SERVER STATS TESTS ============

    #[test]
    fn test_server_stats_new() {
        let stats = ServerStats::new();
        assert_eq!(stats.packets_received.load(Ordering::Relaxed), 0);
        assert_eq!(stats.checksum_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.timeout_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.connection_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.reconnections.load(Ordering::Relaxed), 0);
        assert_eq!(stats.sync_resets.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_server_stats_all_fields() {
        let stats = ServerStats::new();
        
        stats.packets_received.fetch_add(10, Ordering::Relaxed);
        stats.checksum_errors.fetch_add(2, Ordering::Relaxed);
        stats.timeout_errors.fetch_add(3, Ordering::Relaxed);
        stats.connection_errors.fetch_add(4, Ordering::Relaxed);
        stats.reconnections.fetch_add(5, Ordering::Relaxed);
        stats.sync_resets.fetch_add(1, Ordering::Relaxed);
        
        assert_eq!(stats.packets_received.load(Ordering::Relaxed), 10);
        assert_eq!(stats.checksum_errors.load(Ordering::Relaxed), 2);
        assert_eq!(stats.timeout_errors.load(Ordering::Relaxed), 3);
        assert_eq!(stats.connection_errors.load(Ordering::Relaxed), 4);
        assert_eq!(stats.reconnections.load(Ordering::Relaxed), 5);
        assert_eq!(stats.sync_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_server_stats_debug() {
        let stats = ServerStats::new();
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("ServerStats"));
    }

    #[test]
    fn test_server_stats_default() {
        let stats: ServerStats = Default::default();
        assert_eq!(stats.packets_received.load(Ordering::Relaxed), 0);
    }

    // ============ SOCKET TESTS ============

    #[test]
    fn test_create_optimized_socket_invalid_address() {
        let result = create_optimized_socket("invalid_address");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_optimized_socket_connection_refused() {
        let result = create_optimized_socket("127.0.0.1:59999");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_optimized_socket_success() {
        let port = 19001;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            let _ = listener.accept();
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let result = create_optimized_socket(&format!("127.0.0.1:{}", port));
        assert!(result.is_ok());
    }

    // ============ DRAIN BUFFER TESTS ============

    #[test]
    fn test_drain_input_buffer_empty() {
        let port = 19002;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            let _ = listener.accept();
            thread::sleep(Duration::from_millis(200));
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let drained = drain_input_buffer(&mut stream);
        assert_eq!(drained, 0);
    }

    #[test]
    fn test_drain_input_buffer_with_data() {
        let port = 19003;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(b"Hello, World!");
                thread::sleep(Duration::from_millis(200));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        thread::sleep(Duration::from_millis(50));
        let drained = drain_input_buffer(&mut stream);
        assert!(drained > 0);
    }

    #[test]
    fn test_drain_input_buffer_large_data() {
        let port = 19004;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let data = vec![0u8; 3000];
                let _ = stream.write_all(&data);
                thread::sleep(Duration::from_millis(200));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        thread::sleep(Duration::from_millis(100));
        let drained = drain_input_buffer(&mut stream);
        assert!(drained > 0);
        assert!(drained <= 2048 + 512);  // Should stop at limit
    }

    // ============ CONNECT AND AUTH TESTS ============

    #[test]
    fn test_connect_and_auth_success() {
        let port = 19005;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                thread::sleep(Duration::from_millis(200));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let stats = ServerStats::new();
        let result = connect_and_auth(&format!("127.0.0.1:{}", port), "TestServer", &stats);
        assert!(result.is_ok());
        assert_eq!(stats.connection_errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_connect_and_auth_connection_refused() {
        let stats = ServerStats::new();
        let result = connect_and_auth("127.0.0.1:59998", "TestServer", &stats);
        assert!(result.is_err());
    }

    #[test]
    fn test_connect_and_auth_no_response() {
        let port = 19006;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut buf);
                // Don't send response
                thread::sleep(Duration::from_secs(5));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let stats = ServerStats::new();
        let result = connect_and_auth(&format!("127.0.0.1:{}", port), "TestServer", &stats);
        assert!(result.is_err());
        assert!(stats.connection_errors.load(Ordering::Relaxed) > 0);
    }

    // ============ READ EXACT RELIABLE TESTS ============

    #[test]
    fn test_read_exact_reliable_success() {
        let port = 19007;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(b"Hello");
                thread::sleep(Duration::from_millis(100));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        let mut buf = [0u8; 5];
        let result = read_exact_reliable(&mut stream, &mut buf);
        assert!(result.is_ok());
        assert_eq!(&buf, b"Hello");
    }

    #[test]
    fn test_read_exact_reliable_connection_closed() {
        let port = 19008;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);  // Close immediately
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        
        thread::sleep(Duration::from_millis(50));
        
        let mut buf = [0u8; 10];
        let result = read_exact_reliable(&mut stream, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_exact_reliable_partial_then_complete() {
        let port = 19009;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(b"Hel");
                thread::sleep(Duration::from_millis(50));
                let _ = stream.write_all(b"lo");
                thread::sleep(Duration::from_millis(100));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        let mut buf = [0u8; 5];
        let result = read_exact_reliable(&mut stream, &mut buf);
        assert!(result.is_ok());
        assert_eq!(&buf, b"Hello");
    }

    // ============ FETCH DATA TESTS ============

    fn mock_server_with_valid_data(port: u16, is_server1: bool) {
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let mut data = Vec::new();
                    let timestamp: u64 = 1700000000000000;
                    data.extend_from_slice(&timestamp.to_be_bytes());
                    
                    if is_server1 {
                        let temperature: f32 = 23.5;
                        let pressure: i16 = 1013;
                        data.extend_from_slice(&temperature.to_be_bytes());
                        data.extend_from_slice(&pressure.to_be_bytes());
                    } else {
                        let x: i32 = 100;
                        let y: i32 = -200;
                        let z: i32 = 300;
                        data.extend_from_slice(&x.to_be_bytes());
                        data.extend_from_slice(&y.to_be_bytes());
                        data.extend_from_slice(&z.to_be_bytes());
                    }
                    
                    let checksum = calculate_checksum(&data);
                    data.push(checksum);
                    let _ = stream.write_all(&data);
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn test_fetch_server1_valid() {
        let port = 19010;
        mock_server_with_valid_data(port, true);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let stats = ServerStats::new();
        let result = fetch_server1_data(&mut stream, &stats);
        
        assert!(result.is_ok());
        if let Ok(SensorData::TempPressure { temperature, pressure, .. }) = result {
            assert_eq!(temperature, 23.5);
            assert_eq!(pressure, 1013);
        }
        assert_eq!(stats.packets_received.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_fetch_server2_valid() {
        let port = 19011;
        mock_server_with_valid_data(port, false);
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let stats = ServerStats::new();
        let result = fetch_server2_data(&mut stream, &stats);
        
        assert!(result.is_ok());
        if let Ok(SensorData::Accelerometer { x, y, z, .. }) = result {
            assert_eq!(x, 100);
            assert_eq!(y, -200);
            assert_eq!(z, 300);
        }
        assert_eq!(stats.packets_received.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_fetch_server1_checksum_error() {
        let port = 19012;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let mut data = vec![0u8; 14];
                    data.push(255);  // Wrong checksum
                    let _ = stream.write_all(&data);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let stats = ServerStats::new();
        let result = fetch_server1_data(&mut stream, &stats);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Checksum"));
        assert_eq!(stats.checksum_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_fetch_server2_checksum_error() {
        let port = 19013;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let mut data = vec![0u8; 20];
                    data.push(255);  // Wrong checksum
                    let _ = stream.write_all(&data);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let stats = ServerStats::new();
        let result = fetch_server2_data(&mut stream, &stats);
        
        assert!(result.is_err());
        assert_eq!(stats.checksum_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_fetch_server1_invalid_timestamp() {
        let port = 19014;
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let mut data = Vec::new();
                    // Invalid timestamp (too large)
                    data.extend_from_slice(&i64::MAX.to_be_bytes());
                    let temperature: f32 = 23.5;
                    let pressure: i16 = 1013;
                    data.extend_from_slice(&temperature.to_be_bytes());
                    data.extend_from_slice(&pressure.to_be_bytes());
                    let checksum = calculate_checksum(&data);
                    data.push(checksum);
                    let _ = stream.write_all(&data);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let stats = ServerStats::new();
        let result = fetch_server1_data(&mut stream, &stats);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timestamp"));
    }

    // ============ DATA COLLECTION LOOP TESTS ============

    #[test]
    fn test_data_collection_graceful_stop() {
        let port = 19015;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let timestamp: u64 = 1700000000000000;
                    let temperature: f32 = 22.5;
                    let pressure: i16 = 1010;
                    
                    let mut data = Vec::new();
                    data.extend_from_slice(&timestamp.to_be_bytes());
                    data.extend_from_slice(&temperature.to_be_bytes());
                    data.extend_from_slice(&pressure.to_be_bytes());
                    let checksum = calculate_checksum(&data);
                    data.push(checksum);
                    
                    let _ = stream.write_all(&data);
                }
                
                thread::sleep(Duration::from_millis(200));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        let result = data_collection_loop(
            &mut stream,
            true,
            "TestServer",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_ok());
        assert!(stats.packets_received.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn test_data_collection_loop_server2() {
        let port = 19016;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    let timestamp: u64 = 1700000000000000;
                    let x: i32 = 100;
                    let y: i32 = 200;
                    let z: i32 = 300;
                    
                    let mut data = Vec::new();
                    data.extend_from_slice(&timestamp.to_be_bytes());
                    data.extend_from_slice(&x.to_be_bytes());
                    data.extend_from_slice(&y.to_be_bytes());
                    data.extend_from_slice(&z.to_be_bytes());
                    let checksum = calculate_checksum(&data);
                    data.push(checksum);
                    
                    let _ = stream.write_all(&data);
                }
                
                thread::sleep(Duration::from_millis(200));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        let result = data_collection_loop(
            &mut stream,
            false,  // Server 2
            "TestServer2",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_ok());
        assert!(stats.packets_received.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn test_data_collection_checksum_desync() {
        let port = 19017;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                let mut cmd_buf = vec![0u8; GET_CMD.len()];
                if stream.read_exact(&mut cmd_buf).is_ok() {
                    // Send data with wrong checksum
                    let mut data = vec![0u8; 14];
                    data.push(255);
                    let _ = stream.write_all(&data);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let result = data_collection_loop(
            &mut stream,
            true,
            "TestServer",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("desync"));
        assert_eq!(stats.sync_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_data_collection_consecutive_errors() {
        let port = 19018;
        let running = Arc::new(AtomicBool::new(true));
        
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                
                let mut auth_buf = vec![0u8; KEY.len()];
                let _ = stream.read_exact(&mut auth_buf);
                let _ = stream.write_all(b"AUTH_OK\n");
                
                // Don't send data - will cause consecutive timeouts
                thread::sleep(Duration::from_secs(10));
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        
        stream.write_all(KEY).unwrap();
        let mut auth_buf = [0u8; 16];
        stream.read(&mut auth_buf).unwrap();
        
        let result = data_collection_loop(
            &mut stream,
            true,
            "TestServer",
            &writer,
            &stats,
            &running,
        );
        
        assert!(result.is_err());
    }

    // ============ WORKER THREAD TESTS ============

    #[test]
    fn test_worker_thread_connection_refused() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        let running = Arc::new(AtomicBool::new(true));
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        worker_thread(
            "127.0.0.1:59997",
            true,
            writer,
            stats.clone(),
            running,
        );
        
        assert!(stats.connection_errors.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_worker_thread_with_reconnect() {
        let port = 19019;
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats = Arc::new(ServerStats::new());
        let running = Arc::new(AtomicBool::new(true));
        
        thread::spawn(move || {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = vec![0u8; KEY.len()];
                    let _ = stream.read_exact(&mut buf);
                    let _ = stream.write_all(b"AUTH_OK\n");
                    thread::sleep(Duration::from_millis(50));
                    drop(stream);
                }
            }
        });
        
        thread::sleep(Duration::from_millis(50));
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        worker_thread(
            &format!("127.0.0.1:{}", port),
            true,
            writer,
            stats.clone(),
            running,
        );
        
        assert!(stats.reconnections.load(Ordering::Relaxed) > 0);
    }

    // ============ STATS AND FLUSH THREAD TESTS ============

    #[test]
    fn test_stats_and_flush_thread() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        let stats1 = Arc::new(ServerStats::new());
        let stats2 = Arc::new(ServerStats::new());
        let running = Arc::new(AtomicBool::new(true));
        
        stats1.packets_received.store(100, Ordering::Relaxed);
        stats2.packets_received.store(200, Ordering::Relaxed);
        
        {
            let mut w = writer.lock().unwrap();
            w.write_all(b"test data\n").unwrap();
        }
        
        let running_clone = Arc::clone(&running);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(600));
            running_clone.store(false, Ordering::SeqCst);
        });
        
        stats_and_flush_thread(
            writer.clone(),
            stats1,
            stats2,
            running,
        );
        
        // Verify file was flushed
        let metadata = temp_file.as_file().metadata().unwrap();
        assert!(metadata.len() > 0);
    }

    // ============ FILE WRITING TESTS ============

    #[test]
    fn test_file_writing() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        
        let test_data = "2024-01-01 12:00:00 [S1] temperature=25.00C pressure=1013\n";
        {
            let mut w = writer.lock().unwrap();
            w.write_all(test_data.as_bytes()).unwrap();
            w.flush().unwrap();
        }
        
        let metadata = temp_file.as_file().metadata().unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_file_writing_multiple() {
        let temp_file = NamedTempFile::new().unwrap();
        let writer = Arc::new(Mutex::new(BufWriter::new(temp_file.reopen().unwrap())));
        
        for i in 0..10 {
            let line = format!("Line {}\n", i);
            let mut w = writer.lock().unwrap();
            w.write_all(line.as_bytes()).unwrap();
        }
        
        {
            let mut w = writer.lock().unwrap();
            w.flush().unwrap();
        }
        
        let metadata = temp_file.as_file().metadata().unwrap();
        assert!(metadata.len() > 50);
    }

    // ============ ATOMIC OPERATIONS TESTS ============

    #[test]
    fn test_atomic_operations() {
        let running = AtomicBool::new(true);
        assert!(running.load(Ordering::SeqCst));
        
        running.store(false, Ordering::SeqCst);
        assert!(!running.load(Ordering::SeqCst));
    }

    #[test]
    fn test_atomic_u64_operations() {
        let counter = AtomicU64::new(0);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
        
        counter.fetch_add(5, Ordering::Relaxed);
        assert_eq!(counter.load(Ordering::Relaxed), 5);
        
        counter.store(100, Ordering::Relaxed);
        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }

    // ============ EDGE CASES ============

    #[test]
    fn test_extreme_temperature_values() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        
        let cold = SensorData::TempPressure {
            timestamp,
            temperature: -273.15,
            pressure: 0,
        };
        let formatted = format_data(&cold);
        assert!(formatted.contains("-273.15"));
        
        let hot = SensorData::TempPressure {
            timestamp,
            temperature: 1000.0,
            pressure: i16::MAX,
        };
        let formatted = format_data(&hot);
        assert!(formatted.contains("1000.00"));
    }

    #[test]
    fn test_extreme_accelerometer_values() {
        let timestamp = DateTime::from_timestamp_micros(1000000).unwrap();
        
        let data = SensorData::Accelerometer {
            timestamp,
            x: i32::MAX,
            y: i32::MIN,
            z: 0,
        };
        let formatted = format_data(&data);
        assert!(formatted.contains(&i32::MAX.to_string()));
        assert!(formatted.contains(&i32::MIN.to_string()));
    }
}