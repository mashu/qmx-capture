use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{FftPlanner, num_complex::Complex};
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
    cursor::MoveTo,
    event::{self, Event, KeyCode},
};
use std::{
    sync::{Arc, Mutex},
    io::stdout,
    time::Duration,
    thread,
};

const FFT_SIZE: usize = 2048;
const WATERFALL_LINES: usize = 30;
const UPDATE_INTERVAL_MS: u64 = 50;

struct AudioBuffer {
    samples: Vec<f32>,
    position: usize,
}

fn list_devices() -> Result<Vec<cpal::Device>> {
    let host = cpal::default_host();
    let devices = host.input_devices()?;
    println!("Available input devices:");
    println!("----------------------");
    
    let device_list: Vec<_> = devices.collect();
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
    
    Ok(device_list)
}

fn get_user_device_choice(max: usize) -> usize {
    loop {
        println!("\nSelect device number (0-{}): ", max - 1);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if let Ok(num) = input.trim().parse() {
            if num < max {
                return num;
            }
        }
        println!("Invalid selection, try again");
    }
}

fn main() -> Result<()> {
    let device_list = list_devices()?;
    if device_list.is_empty() {
        println!("No input devices found!");
        return Ok(());
    }
    
    let device_idx = get_user_device_choice(device_list.len());
    let device = &device_list[device_idx];
    
    let config = device.default_input_config()?;
    println!("\nUsing device: {} @ {} Hz", device.name()?, config.sample_rate().0);
    
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    
    let buffer = Arc::new(Mutex::new(AudioBuffer {
        samples: vec![0.0; FFT_SIZE],
        position: 0,
    }));
    
    let buffer_clone = Arc::clone(&buffer);
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buffer = buffer_clone.lock().unwrap();
            for &sample in data {
                let pos = buffer.position;
                buffer.samples[pos] = sample;
                buffer.position = (pos + 1) % FFT_SIZE;
            }
        },
        |err| eprintln!("Error in stream: {}", err),
        None,
    )?;
    
    stream.play()?;
    
    let mut waterfall = vec![vec![0.0f32; FFT_SIZE/2]; WATERFALL_LINES];
    let mut line = 0;
    
    println!("\nPress 'q' to exit");
    
    loop {
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
        
        let samples = {
            let buffer = buffer.lock().unwrap();
            let mut samples = buffer.samples.clone();
            for i in 0..FFT_SIZE {
                let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos());
                samples[i] *= window;
            }
            samples
        };
        
        let mut fft_buffer: Vec<Complex<f32>> = samples.iter()
            .map(|&x| Complex::new(x, 0.0))
            .collect();
        
        fft.process(&mut fft_buffer);
        
        let spectrum: Vec<f32> = fft_buffer.iter()
            .take(FFT_SIZE/2)
            .map(|x| (x.norm_sqr() as f32).sqrt())
            .collect();
        
        waterfall[line] = spectrum;
        line = (line + 1) % WATERFALL_LINES;
        
        execute!(stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
        
        for i in 0..WATERFALL_LINES {
            let row = (line + i) % WATERFALL_LINES;
            for &magnitude in waterfall[row].iter().step_by(8) {
                let normalized = (magnitude * 50.0).min(1.0);
                let intensity = (normalized * 9.0) as u8;
                print!("{}", match intensity {
                    0 => " ",
                    1..=3 => ".",
                    4..=6 => "+",
                    7..=8 => "#",
                    _ => "@",
                });
            }
            println!();
        }
        
        thread::sleep(Duration::from_millis(UPDATE_INTERVAL_MS));
    }
    
    Ok(())
}
