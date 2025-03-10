use std::io::{self, Read, Write};
use std::time::Duration;
use std::sync::mpsc;
use std::thread;

use serialport::{SerialPort, SerialPortType};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

fn main() -> io::Result<()> {
    // List available serial ports
    let available_ports = match serialport::available_ports() {
        Ok(ports) => ports,
        Err(e) => {
            eprintln!("Error listing serial ports: {}", e);
            return Ok(());
        }
    };

    if available_ports.is_empty() {
        println!("No serial ports found.");
        return Ok(());
    }

    // Format and display available serial ports in a table sized for 80x24 terminal
    println!("\nAvailable serial ports:");
    println!("┌─────┬──────────────────┬──────────┬──────────────────────────┐");
    println!("│ Idx │ Port Name        │ Type     │ Details                  │");
    println!("├─────┼──────────────────┼──────────┼──────────────────────────┤");
    
    for (i, port) in available_ports.iter().enumerate() {
        let port_name = format!("{}", port.port_name);
        let port_name = if port_name.len() > 16 { 
            format!("{}...", &port_name[0..13]) 
        } else { 
            format!("{:<16}", port_name) 
        };
        
        let (port_type, details) = match &port.port_type {
            SerialPortType::UsbPort(info) => {
                ("USB", format!("VID:{:04x} PID:{:04x}", 
                    info.vid, info.pid))
            }
            SerialPortType::BluetoothPort => {
                ("Bluetooth", String::from("N/A"))
            }
            SerialPortType::PciPort => {
                ("PCI", String::from("N/A"))
            }
            _ => {
                ("Unknown", String::from("N/A"))
            }
        };
        
        let details = if details.len() > 24 { 
            format!("{}...", &details[0..21]) 
        } else { 
            format!("{:<24}", details) 
        };
        
        println!("│ {:3} │ {} │ {:<8} │ {} │", 
                 i, port_name, port_type, details);
    }
    
    println!("└─────┴──────────────────┴──────────┴──────────────────────────┘");

    print!("Select port [0-{}]: ", available_ports.len() - 1);
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let port_index = input.trim().parse::<usize>().unwrap_or(0);
    
    if port_index >= available_ports.len() {
        println!("Invalid selection, using port 0.");
    }
    
    let port_name = &available_ports[port_index.min(available_ports.len() - 1)].port_name;
    
    println!("\nAvailable baud rates: 9600, 19200, 38400, 57600, 115200");
    print!("Select baud rate [115200]: ");
    io::stdout().flush()?;
    
    input.clear();
    io::stdin().read_line(&mut input)?;
    let baud_rate = input.trim().parse::<u32>().unwrap_or(115200);
    
    println!("Opening {} at {} baud", port_name, baud_rate);
    
    // Open the serial port
    let port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(10))
        .open();

    let mut port = match port {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to open port: {}", e);
            return Ok(());
        }
    };

    println!("Serial port opened successfully.");
    println!("Press Ctrl+X to exit, Ctrl+T for command mode.");
    println!("Command mode: 'b' to change baud rate, 'c' to clear screen");
    thread::sleep(Duration::from_millis(1000));

    // Set up terminal
    let stdout = io::stdout().into_raw_mode()?;
    let mut stdout = io::BufWriter::new(stdout);

    // Set up channels for communication between threads
    let (tx, rx) = mpsc::channel();

    // Thread for reading keyboard input
    let tx_clone = tx.clone();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut keys = stdin.keys();
        while let Some(result) = keys.next() {
            if let Ok(key) = result {
                if tx_clone.send(key).is_err() {
                    break;
                }
                
                // Exit if Ctrl+X is pressed
                if key == Key::Ctrl('x') {
                    break;
                }
            }
        }
    });

    // Thread for reading from serial port
    let tx_clone = tx.clone();
    let mut port_clone = match serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(10))
        .open() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to open port clone: {}", e);
            return Ok(());
        }
    };

    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match port_clone.read(&mut buffer) {
                Ok(count) if count > 0 => {
                    for i in 0..count {
                        let _ = tx_clone.send(Key::Char(buffer[i] as char));
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::TimedOut => {
                    // Do nothing on timeout
                }
                Err(e) => {
                    eprintln!("Error reading from serial port: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    let mut command_mode = false;

    // Main loop
    loop {
        match rx.recv() {
            Ok(Key::Ctrl('x')) => {
                write!(stdout, "\r\nExiting...\r\n")?;
                stdout.flush()?;
                break;
            }
            Ok(Key::Ctrl('t')) => {
                command_mode = true;
                write!(stdout, "\r\n[Command Mode] ")?;
                stdout.flush()?;
            }
            Ok(key) => {
                if command_mode {
                    match key {
                        Key::Char('b') => {
                            write!(stdout, "\r\nEnter new baud rate: ")?;
                            stdout.flush()?;
                            let mut baud_input = String::new();
                            io::stdin().read_line(&mut baud_input)?;
                            
                            if let Ok(new_baud) = baud_input.trim().parse::<u32>() {
                                write!(stdout, "\r\nChanging baud rate to {}\r\n", new_baud)?;
                                port = match serialport::new(port_name, new_baud)
                                    .timeout(Duration::from_millis(10))
                                    .open() {
                                    Ok(p) => p,
                                    Err(e) => {
                                        write!(stdout, "\r\nFailed to change baud rate: {}\r\n", e)?;
                                        port
                                    }
                                };
                            } else {
                                write!(stdout, "\r\nInvalid baud rate\r\n")?;
                            }
                        }
                        Key::Char('c') => {
                            write!(stdout, "\x1B[2J\x1B[1;1H")?; // Clear screen and move cursor to top
                        }
                        _ => {
                            write!(stdout, "\r\nUnknown command\r\n")?;
                        }
                    }
                    command_mode = false;
                    write!(stdout, "[Terminal Mode]\r\n")?;
                    stdout.flush()?;
                } else {
                    match key {
                        Key::Char(c) => {
                            // Send character to serial port
                            if let Err(e) = port.write_all(&[c as u8]) {
                                write!(stdout, "\r\nError writing to port: {}\r\n", e)?;
                            } else {
                                // Echo character to terminal
                                write!(stdout, "{}", c)?;
                                stdout.flush()?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => {
                // Channel closed or error
                break;
            }
        }
    }

    Ok(())
}
