// src/main.rs
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

fn main() -> Result<()> {
    // Temporarily redirect stderr to /dev/null
    let stderr_backup = unsafe { libc::dup(2) };
    let devnull = unsafe { libc::open("/dev/null\0".as_ptr() as *const i8, libc::O_WRONLY) };
    unsafe { libc::dup2(devnull, 2) };

    // Get devices
    let host = cpal::default_host();
    let devices = host.input_devices()?;
    let device_list: Vec<_> = devices.collect();

    // Restore stderr
    unsafe {
        libc::dup2(stderr_backup, 2);
        libc::close(devnull);
        libc::close(stderr_backup);
    }

    // Print devices
    println!("Input Devices:");
    println!("-------------");
    
    for (idx, device) in device_list.iter().enumerate() {
        if let Ok(name) = device.name() {
            print!("{}. {}", idx, name);
            if let Ok(config) = device.default_input_config() {
                println!(" ({} ch, {} Hz)", 
                    config.channels(),
                    config.sample_rate().0);
            } else {
                println!();
            }
        }
    }

    Ok(())
}
