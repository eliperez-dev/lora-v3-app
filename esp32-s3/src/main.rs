use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration as WifiConfiguration, EspWifi};


const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");


fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

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

    // 2. Create shared counter state
    let counter = Arc::new(Mutex::new(0u32));

    // 3. Setup HTTP Server
    let mut server = EspHttpServer::new(&Configuration::default())?;

    // GET /count
    let counter_get = counter.clone();
    server.fn_handler("/count", Method::Get, move |request| -> anyhow::Result<()> {
        let count = *counter_get.lock().unwrap();
        let html = format!("{}", count);
        let mut response = request.into_response(200, Some("OK"), &[("Access-Control-Allow-Origin", "*")])?;
        response.write(html.as_bytes())?;
        Ok(())
    })?;

    // POST /add
    let counter_add = counter.clone();
    server.fn_handler("/add", Method::Post, move |request| -> anyhow::Result<()> {
        let mut count = counter_add.lock().unwrap();
        *count += 1;
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
