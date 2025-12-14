use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration as WifiConfiguration, EspWifi};

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Spawn a thread with a larger stack size
    let builder = thread::Builder::new().stack_size(20 * 1024);

    let handle = builder.spawn(|| {
        run_app().unwrap();
    })?;

    handle.join().unwrap();
    Ok(())
}

fn run_app() -> anyhow::Result<()> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // 1. Setup Wi-Fi
    let mut wifi = EspWifi::new(peripherals.modem, sys_loop, Some(nvs))?;
    wifi.set_configuration(&WifiConfiguration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    
    // Wait for connection
    while !wifi.is_up()? {
        let config = wifi.get_configuration()?;
        log::info!("Waiting for station {:?}", config);
        thread::sleep(Duration::from_millis(1000));
    }
    log::info!("Wifi connected!");

    // 1.5 Power up the board (Vext) - Critical for Heltec V3
    // GPIO 36 controls power to OLED, LoRa, etc. Low = On.
    let mut vext = PinDriver::output(peripherals.pins.gpio36)?;
    vext.set_low()?;
    thread::sleep(Duration::from_millis(100));

    // 2. Setup OLED Display (Heltec V3: SDA=17, SCL=18, RST=21)
    let i2c = peripherals.i2c0;
    let sda = peripherals.pins.gpio17;
    let scl = peripherals.pins.gpio18;
    let config = I2cConfig::new().baudrate(100.kHz().into());
    let mut i2c_driver = I2cDriver::new(i2c, sda, scl, &config)?;

    // Reset the OLED
    let mut rst = PinDriver::output(peripherals.pins.gpio21)?;
    rst.set_low()?;
    thread::sleep(Duration::from_millis(100));
    rst.set_high()?;
    thread::sleep(Duration::from_millis(100));

    // Scan I2C
    log::info!("Scanning I2C...");
    for addr in 0x00..0x7F {
        if i2c_driver.write(addr, &[], 100).is_ok() {
            log::info!("Found I2C device at address: 0x{:02X}", addr);
        }
    }

    let interface = I2CDisplayInterface::new_custom_address(i2c_driver, 0x3C);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    
    display.init().unwrap();

    // Debug: Flash screen
    for _ in 0..5 {
        display.clear(BinaryColor::On).unwrap();
        display.flush().unwrap();
        thread::sleep(Duration::from_millis(200));
        
        display.clear(BinaryColor::Off).unwrap();
        display.flush().unwrap();
        thread::sleep(Duration::from_millis(200));
    }

    // Initial draw
    draw_count(&mut display, 0);
    display.flush().unwrap();

    // 3. Create shared state
    let counter = Arc::new(Mutex::new(0u32));
    let display = Arc::new(Mutex::new(display));

    // 4. Setup HTTP Server
    let mut server = EspHttpServer::new(&Configuration::default())?;

    // GET /count
    let counter_get = counter.clone();
    server.fn_handler("/count", Method::Get, move |request| -> anyhow::Result<()> {
        let count = *counter_get.lock().unwrap();
        log::info!("GET /count request received. Current count: {}", count);
        let html = format!("{}", count);
        let mut response = request.into_response(200, Some("OK"), &[("Access-Control-Allow-Origin", "*")])?;
        response.write(html.as_bytes())?;
        Ok(())
    })?;

    // POST /add
    let counter_add = counter.clone();
    let display_add = display.clone();
    server.fn_handler("/add", Method::Post, move |request| -> anyhow::Result<()> {
        let mut count = counter_add.lock().unwrap();
        *count += 1;
        
        // Update Display
        if let Ok(mut disp) = display_add.lock() {
            draw_count(&mut *disp, *count);
            let _ = disp.flush();
        }

        let html = format!("Added. New count: {}", *count);
        let mut response = request.into_response(200, Some("OK"), &[("Access-Control-Allow-Origin", "*")])?;
        response.write(html.as_bytes())?;
        Ok(())
    })?;

    log::info!("Server running...");

    // Keep the main thread alive
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn draw_count<D>(display: &mut D, count: u32) 
where D: DrawTarget<Color = BinaryColor> {
    let _ = display.clear(BinaryColor::Off);
    
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let _ = Text::with_baseline("Counter:", Point::new(0, 0), text_style, Baseline::Top)
        .draw(display);

    let count_str = format!("{}", count);
    let _ = Text::with_baseline(&count_str, Point::new(0, 20), text_style, Baseline::Top)
        .draw(display);

    // display.flush() is not part of DrawTarget, it's specific to the display driver.
    // Since we are using BufferedGraphicsMode, we need to flush.
    // However, we can't easily bound D to have flush() without complex trait bounds.
    // For now, we will rely on the fact that we are passing the specific type in main,
    // but here we are generic.
    //
    // To fix this properly, we should probably not make this function generic 
    // or use a trait that includes flush.
}
 