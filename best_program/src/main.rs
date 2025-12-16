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
    TempHumidity {
        timestamp: DateTime<Utc>,
        temperature: f32,
        humidity: f32,
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
    
    let temp_raw = f32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let hum_raw = i16::from_be_bytes([data[12], data[13]]);
    
    let timestamp = DateTime::from_timestamp_micros(timestamp_raw as i64)
        .ok_or("Invalid timestamp")?;
    let temperature = temp_raw / 100.0;
    let humidity = hum_raw as f32 / 100.0;
    
    Ok(SensorData::TempHumidity {
        timestamp,
        temperature,
        humidity,
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
    stats: &Arc<Mutex<(u64, u64)>>, // (server1_count, server2_count)
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
                error_count = 0; // Сбрасываем счетчик ошибок при успехе
                
                let line = match data {
                    SensorData::TempHumidity {
                        timestamp,
                        temperature,
                        humidity,
                    } => {
                        format!(
                            "{} [S1] temperature={:.2}C humidity={:.2}%\n",
                            timestamp.format("%Y-%m-%d %H:%M:%S"),
                            temperature,
                            humidity
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
                
                // Записываем в файл
                if let Ok(mut w) = writer.lock() {
                    if let Err(e) = w.write_all(line.as_bytes()) {
                        eprintln!("[{}] ✗ Write error: {}", server_name, e);
                    }
                }
                
                // Обновляем статистику
                if let Ok(mut s) = stats.lock() {
                    if is_server1 {
                        s.0 = packet_count;
                    } else {
                        s.1 = packet_count;
                    }
                }
                
                // Небольшая задержка для предотвращения перегрузки
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                error_count += 1;
                eprintln!("[{}] ✗ Data fetch error ({}): {}", server_name, error_count, e);
                
                // После нескольких ошибок подряд - разрываем соединение
                if error_count >= 3 {
                    return Err(format!("Too many consecutive errors: {}", error_count).into());
                }
                
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    
    // Ok при корректном завершении по Ctrl+C
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
                
                // Начинаем сбор данных
                match data_collection_loop(&mut stream, is_server1, server_name, &writer, &stats, &running) {
                    Ok(_) => {
                        // Нормальное завершение из-за Ctrl+C
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
        
        // Если Ctrl+C не нажат, пытаемся переподключиться
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

fn main() {
    println!("Starting network data logger...");

    println!("Server 1: {} (Temperature/Humidity)", SERVER1);
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

    // Запускаем поток для Server 1
    let writer1 = Arc::clone(&writer);
    let stats1 = Arc::clone(&stats);
    let running1 = Arc::clone(&running);
    let handle1 = thread::spawn(move || {
        worker_thread(SERVER1, true, writer1, stats1, running1);
    });
    
    // Запускаем поток для Server 2
    let writer2 = Arc::clone(&writer);
    let stats2 = Arc::clone(&stats);
    let running2 = Arc::clone(&running);
    let handle2 = thread::spawn(move || {
        worker_thread(SERVER2, false, writer2, stats2, running2);
    });
    
    // Поток для периодической записи буфера на диск и вывода статистики
    let writer3 = Arc::clone(&writer);
    let stats3 = Arc::clone(&stats);
    let running3 = Arc::clone(&running);
    let handle3 = thread::spawn(move || {
        while running3.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(10));

            // Сброс буфера на диск
            if let Ok(mut w) = writer3.lock() {
                if let Err(e) = w.flush() {
                    eprintln!("✗ Flush error: {}", e);
                }
            }
            
            // Вывод статистики
            if let Ok(s) = stats3.lock() {
                println!("\n[STATS] Server1: {} packets | Server2: {} packets", s.0, s.1);
            }
        }
        
        // Финальный сброс буфера при завершении
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