use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait};

fn main() -> Result<()> {
    let host = cpal::default_host();
    let devices = host.input_devices()?;
    let device_list: Vec<_> = devices.collect();

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
