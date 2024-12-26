// src/main.rs
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::io::{self, Write};

fn select_device(host: &cpal::Host) -> Result<cpal::Device> {
    let devices = host.input_devices()?;
    let devices: Vec<cpal::Device> = devices.collect();
    
    println!("\nAvailable input devices:");
    for (i, device) in devices.iter().enumerate() {
        let name = device.name()?;
        println!("{}: {}", i, name);
        
        // Print device properties
        if let Ok(config) = device.default_input_config() {
            println!("  Default config:");
            println!("    Channels: {}", config.channels());
            println!("    Sample Rate: {} Hz", config.sample_rate().0);
            println!("    Sample Format: {:?}", config.sample_format());
        }
        println!();
    }

    loop {
        print!("Select device (0-{}): ", devices.len() - 1);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().parse::<usize>() {
            Ok(index) if index < devices.len() => {
                return Ok(devices.into_iter().nth(index).unwrap());
            }
            _ => println!("Invalid selection, please try again."),
        }
    }
}

fn main() -> Result<()> {
    // Get the default host
    let host = cpal::default_host();
    println!("Using audio host: {}", host.id().name());

    // Let user select input device
    let device = select_device(&host)?;
    println!("\nSelected device: {}", device.name()?);

    // Get the default configuration
    let config = device
        .default_input_config()
        .context("Failed to get default input config")?;
    println!("Using config: {:?}", config);

    // Create a flag to control the recording
    let recording = Arc::new(AtomicBool::new(true));
    let recording_clone = Arc::clone(&recording);

    // Set up the audio input stream based on the sample format
    let stream = match config.sample_format() {
        SampleFormat::F32 => build_input_stream::<f32>(&device, &config.into())?,
        SampleFormat::I16 => build_input_stream::<i16>(&device, &config.into())?,
        SampleFormat::U16 => build_input_stream::<u16>(&device, &config.into())?,
        sample_format => {
            return Err(anyhow::Error::msg(format!(
                "Unsupported sample format: {:?}",
                sample_format
            )))
        }
    };

    // Start the stream
    stream.play()?;
    println!("\nRecording... Press Ctrl+C to stop.");

    // Wait for Ctrl+C
    ctrlc::set_handler(move || {
        recording_clone.store(false, Ordering::SeqCst);
    })?;

    while recording.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) -> Result<cpal::Stream>
where
    T: Sample + Send + 'static + cpal::SizedSample,
{
    let channels = config.channels as usize;

    let err_fn = |err| eprintln!("An error occurred on the input audio stream: {}", err);

    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for frame in data.chunks(channels) {
                // Convert samples to f32 for RMS calculation
                let sum: f32 = frame.iter()
                    .map(|&sample| {
                        let float_sample = Sample::to_float_sample(sample);
                        float_sample.to_sample::<f32>() // Convert to f32 using to_sample
                    })
                    .map(|s| s * s)
                    .sum();
                
                let rms = (sum / channels as f32).sqrt();
                println!("RMS: {:.6}", rms);
            }
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}
