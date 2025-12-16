use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_svc::hal::spi::{SpiDriver, SpiDeviceDriver, config::Config as SpiConfig, config::DriverConfig};
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

    
    // 1.0 Power up the board (Vext) - Critical for Heltec V3
    // GPIO 36 controls power to OLED, LoRa, etc. Low = On.
    let mut vext = PinDriver::output(peripherals.pins.gpio36)?;
    vext.set_low()?;
    thread::sleep(Duration::from_millis(100));

    // 1.5. Setup OLED Display (Heltec V3: SDA=17, SCL=18, RST=21)
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

    draw_startup_screen(&mut display, "Initilizing...");
    display.flush().unwrap();


    // 2. Setup Wi-Fi
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

    let ip_info = wifi.sta_netif().get_ip_info()?;
    let ip = ip_info.ip;
    log::info!("IP Address: {}", ip); 


    // 2.5 Setup LoRa (Heltec V3)
    log::info!("Setting up LoRa...");
    let sck = peripherals.pins.gpio9;
    let mosi = peripherals.pins.gpio10;
    let miso = peripherals.pins.gpio11;
    let cs = peripherals.pins.gpio8;
    let rst = peripherals.pins.gpio12;
    let busy = peripherals.pins.gpio13;
    let dio1 = peripherals.pins.gpio14;

    let spi = peripherals.spi2;
    let spi_config = SpiConfig::new().baudrate(2.MHz().into());
    let spi_driver = SpiDriver::new(spi, sck, mosi, Some(miso), &DriverConfig::default())?;
    let spi_device = SpiDeviceDriver::new(spi_driver, Some(cs), &spi_config)?;

    let rst_pin = PinDriver::output(rst)?;
    let busy_pin = PinDriver::input(busy)?;
    let dio1_pin = PinDriver::input(dio1)?;

    let mut lora = sx126x::SX126x::new(spi_device, (rst_pin, busy_pin, PinDriver::output(peripherals.pins.gpio34)?, dio1_pin));
    
    let lora_mod_params = sx126x::op::LoraModParams::default()
        .set_spread_factor(sx126x::op::LoRaSpreadFactor::SF7)
        .set_bandwidth(sx126x::op::LoRaBandWidth::BW125)
        .set_coding_rate(sx126x::op::LoraCodingRate::CR4_5);
    let mod_params: sx126x::op::ModParams = lora_mod_params.into();

    let lora_packet_params = sx126x::op::LoRaPacketParams::default()
        .set_preamble_len(8)
        .set_header_type(sx126x::op::LoRaHeaderType::VarLen)
        .set_payload_len(255)
        .set_crc_type(sx126x::op::LoRaCrcType::CrcOn)
        .set_invert_iq(sx126x::op::LoRaInvertIq::Standard);
    let packet_params: sx126x::op::PacketParams = lora_packet_params.into();

    let tx_params = sx126x::op::TxParams::default()
        .set_power_dbm(22)
        .set_ramp_time(sx126x::op::RampTime::Ramp10u);
    
    let pa_config = sx126x::op::PaConfig::default()
        .set_device_sel(sx126x::op::DeviceSel::SX1262)
        .set_pa_duty_cycle(0x04)
        .set_hp_max(0x07);

    let rf_frequency = 915_000_000;
    let rf_freq = sx126x::calc_rf_freq(915.0, 32.0);

    let lora_config = sx126x::conf::Config {
        packet_type: sx126x::op::PacketType::LoRa,
        sync_word: 0x1424,
        calib_param: sx126x::op::CalibParam::all(),
        mod_params,
        pa_config,
        packet_params: Some(packet_params),
        tx_params,
        dio1_irq_mask: sx126x::op::IrqMask::all(),
        dio2_irq_mask: sx126x::op::IrqMask::none(),
        dio3_irq_mask: sx126x::op::IrqMask::none(),
        rf_freq,
        rf_frequency,
        tcxo_opts: Some((sx126x::op::TcxoVoltage::Volt1_7, sx126x::op::TcxoDelay::from_ms(10))),
    };
    
    lora.init(lora_config).map_err(|e| anyhow::anyhow!("LoRa init failed: {:?}", e))?;
    
    // Set RX mode
    lora.set_rx(sx126x::op::RxTxTimeout::continuous_rx()).map_err(|e| anyhow::anyhow!("Set RX failed: {:?}", e))?;

    log::info!("LoRa initialized!");

    // Initial draw
    draw_count(&mut display, 0, &ip);
    display.flush().unwrap();


    // 3. Create shared state
    let counter = Arc::new(Mutex::new(0u32));
    let display = Arc::new(Mutex::new(display));
    let lora = Arc::new(Mutex::new(lora));

    // 3.5 Start LoRa Receiver Thread
    let counter_rx = counter.clone();
    let display_rx = display.clone();
    let lora_rx = lora.clone();
    
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            
            if let Ok(mut lora) = lora_rx.lock() {
                if let Ok(status) = lora.get_irq_status() {
                    if status.rx_done() {
                        if let Ok(rx_status) = lora.get_rx_buffer_status() {
                            let len = rx_status.payload_length_rx();
                            let ptr = rx_status.rx_start_buffer_pointer();
                            let mut buffer = vec![0u8; len as usize];
                            if lora.read_buffer(ptr, &mut buffer).is_ok() {
                                if let Ok(msg) = std::str::from_utf8(&buffer) {
                                    log::info!("Received LoRa packet: {}", msg);
                                    if msg.starts_with("Count: ") {
                                        if let Ok(new_count) = msg["Count: ".len()..].parse::<u32>() {
                                            if let Ok(mut count) = counter_rx.lock() {
                                                *count = new_count;
                                                if let Ok(mut disp) = display_rx.lock() {
                                                    draw_count(&mut *disp, *count, &ip);
                                                    let _ = disp.flush();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _ = lora.clear_irq_status(sx126x::op::irq::IrqMask::none().combine(sx126x::op::irq::IrqMaskBit::RxDone));
                    }
                    
                    if status.tx_done() {
                        let _ = lora.clear_irq_status(sx126x::op::irq::IrqMask::none().combine(sx126x::op::irq::IrqMaskBit::TxDone));
                        let _ = lora.set_rx(sx126x::op::rxtx::RxTxTimeout::continuous_rx());
                    }
                }
            }
        }
    });

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
    let lora_add = lora.clone();
    server.fn_handler("/add", Method::Post, move |request| -> anyhow::Result<()> {
        let mut count = counter_add.lock().unwrap();
        *count += 1;
        
        // Update Display
        if let Ok(mut disp) = display_add.lock() {
            draw_count(&mut *disp, *count, &ip);
            let _ = disp.flush();
        }

        // Send LoRa
        if let Ok(mut lora) = lora_add.lock() {
            let msg = format!("Count: {}", *count);
            let buffer = msg.as_bytes();
            
            let lora_packet_params = sx126x::op::packet::LoRaPacketParams::default()
                .set_preamble_len(8)
                .set_header_type(sx126x::op::packet::LoRaHeaderType::VarLen)
                .set_payload_len(buffer.len() as u8)
                .set_crc_type(sx126x::op::packet::LoRaCrcType::CrcOn)
                .set_invert_iq(sx126x::op::packet::LoRaInvertIq::Standard);
            let packet_params: sx126x::op::PacketParams = lora_packet_params.into();
            
            if let Err(e) = lora.set_packet_params(packet_params) {
                log::error!("Failed to set packet params: {:?}", e);
            } else if let Err(e) = lora.write_buffer(0, buffer) {
                log::error!("Failed to write buffer: {:?}", e);
            } else if let Err(e) = lora.set_tx(sx126x::op::rxtx::RxTxTimeout::from_ms(2000)) { // 2s timeout
                log::error!("Failed to set TX: {:?}", e);
            } else {
                log::info!("Sent LoRa packet: {}", msg);
            }
        }

        let html = format!("Added. New count: {}", *count);
        let mut response = request.into_response(200, Some("OK"), &[("Access-Control-Allow-Origin", "*")])?;
        response.write(html.as_bytes())?;
        Ok(())
    })?;

    let counter_sub = counter.clone();
    let display_sub = display.clone();
    let lora_sub = lora.clone();
    server.fn_handler("/sub", Method::Post, move |request| -> anyhow::Result<()> {
        let mut count = counter_sub.lock().unwrap();
        *count -= 1;
        
        // Update Display
        if let Ok(mut disp) = display_sub.lock() {
            draw_count(&mut *disp, *count, &ip);
            let _ = disp.flush();
        }

        // Send LoRa
        if let Ok(mut lora) = lora_sub.lock() {
            let msg = format!("Count: {}", *count);
            let buffer = msg.as_bytes();
            
            let lora_packet_params = sx126x::op::packet::LoRaPacketParams::default()
                .set_preamble_len(8)
                .set_header_type(sx126x::op::packet::LoRaHeaderType::VarLen)
                .set_payload_len(buffer.len() as u8)
                .set_crc_type(sx126x::op::packet::LoRaCrcType::CrcOn)
                .set_invert_iq(sx126x::op::packet::LoRaInvertIq::Standard);
            let packet_params: sx126x::op::PacketParams = lora_packet_params.into();
            
            if let Err(e) = lora.set_packet_params(packet_params) {
                log::error!("Failed to set packet params: {:?}", e);
            } else if let Err(e) = lora.write_buffer(0, buffer) {
                log::error!("Failed to write buffer: {:?}", e);
            } else if let Err(e) = lora.set_tx(sx126x::op::rxtx::RxTxTimeout::from_ms(2000)) {
                log::error!("Failed to set TX: {:?}", e);
            } else {
                log::info!("Sent LoRa packet: {}", msg);
            }
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

// Screen size is 128x64

fn draw_count<D>(display: &mut D, count: u32, ip: &esp_idf_svc::ipv4::Ipv4Addr) 
where D: DrawTarget<Color = BinaryColor> {
    let _ = display.clear(BinaryColor::Off);

    // let _ = embedded_graphics::primitives::Circle::new(Point::new(128-32, 0), 32)
    // .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
    // .draw(display); 
    
    let text_style: embedded_graphics::mono_font::MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let _ = Text::with_baseline(&format!("Counter: {}", count), Point::new(0, 0), text_style, Baseline::Top)
        .draw(display);

    let _ = Text::with_baseline(&format!("{:?}", ip), Point::new(0, 55), text_style, Baseline::Top)
        .draw(display);
}


fn draw_startup_screen<D>(display: &mut D, text: &str) 
where D: DrawTarget<Color = BinaryColor> {
    let _ = display.clear(BinaryColor::Off);

    let text_style: embedded_graphics::mono_font::MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();

    let _ = Text::with_baseline(text, Point::new(0, 0), text_style, Baseline::Top)
        .draw(display);

}
 