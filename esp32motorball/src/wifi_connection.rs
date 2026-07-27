use crate::hardware_initialisation::HardwareControls;
use embassy_executor::Spawner;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_println::println;
use esp_radio::wifi::{
    sta::StationConfig, AuthenticationMethod, Config as WifiConfig, ControllerConfig, Interface,
    PowerSaveMode, WifiController,
};
use reqwless::client::HttpClient;
use static_cell::StaticCell;

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASSWORD");

const TX_POWER_QUARTER_DBM: i8 = 44;

pub type EspTcpClient = TcpClient<'static, 1, 1500, 1500>;
pub type EspDnsSocket = DnsSocket<'static>;
pub type EspHttpClient = HttpClient<'static, EspTcpClient, EspDnsSocket>;

static CONTROLLER: StaticCell<WifiController<'static>> = StaticCell::new();
static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static TCP_STATE: StaticCell<TcpClientState<1, 1500, 1500>> = StaticCell::new();
static TCP_CLIENT: StaticCell<EspTcpClient> = StaticCell::new();
static DNS_SOCKET: StaticCell<EspDnsSocket> = StaticCell::new();
static HTTP_CLIENT: StaticCell<EspHttpClient> = StaticCell::new();

pub enum HttpConnection {
    NotConnected,
    Connected(&'static mut EspHttpClient),
}

pub struct ConnectionControls {
    pub http: HttpConnection,
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_task(controller: &'static mut WifiController<'static>) -> ! {
    loop {
        let _ = controller.wait_for_disconnect_async().await;
        println!("wifi disconnected, reconnecting");
        loop {
            match controller.connect_async().await {
                Ok(info) => {
                    println!("wifi reconnected: {info:?}");
                    break;
                }
                Err(e) => {
                    println!("wifi reconnect failed: {e:?}");
                    Timer::after(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

pub async fn connect_wifi(
    spawner: Spawner,
    mut hardware: HardwareControls,
) -> (HardwareControls, ConnectionControls) {
    let Some(wifi_peripheral) = hardware.wifi.take() else {
        return (
            hardware,
            ConnectionControls {
                http: HttpConnection::NotConnected,
            },
        );
    };

    let station_config = WifiConfig::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into())
            .with_auth_method(AuthenticationMethod::Wpa2Personal),
    );

    println!("Starting WiFi");

    let controller = match WifiController::new(
        wifi_peripheral,
        ControllerConfig::default().with_initial_config(station_config),
    ) {
        Ok(c) => CONTROLLER.init(c),
        Err(e) => {
            println!("Failed to create wifi controller: {e:?}");
            return (
                hardware,
                ConnectionControls {
                    http: HttpConnection::NotConnected,
                },
            );
        }
    };

    if let Err(e) = controller.set_power_saving(PowerSaveMode::Maximum) {
        println!("failed to set wifi power save: {e:?}");
    }
    if let Err(e) = controller.set_max_tx_power(TX_POWER_QUARTER_DBM) {
        println!("failed to cap wifi tx power: {e:?}");
    }

    println!("Wifi configured");

    match controller.connect_async().await {
        Ok(info) => println!("Wifi connected: {info:?}"),
        Err(e) => {
            println!("Failed to connect to wifi: {e:?}");
            return (
                hardware,
                ConnectionControls {
                    http: HttpConnection::NotConnected,
                },
            );
        }
    }

    println!("starting network stack");
    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let seed =
        ((hardware.rng.random() as u64) << 32) | hardware.rng.random() as u64;

    let station = Interface::station();
    let resources = STACK_RESOURCES.init(StackResources::new());
    let (stack, runner) = embassy_net::new(station, net_config, resources, seed);

    match net_task(runner) {
        Ok(token) => spawner.spawn(token),
        Err(e) => {
            println!("failed to spawn net_task: {e:?}");
            return (
                hardware,
                ConnectionControls {
                    http: HttpConnection::NotConnected,
                },
            );
        }
    }

    match wifi_task(controller) {
        Ok(token) => spawner.spawn(token),
        Err(e) => println!("failed to spawn wifi_task: {e:?}"),
    }

    println!("waiting for network stack");
    stack.wait_config_up().await;
    println!("network stack is up");

    let tcp_state = TCP_STATE.init(TcpClientState::new());
    let tcp_client = TCP_CLIENT.init(TcpClient::new(stack, tcp_state));
    let dns_socket = DNS_SOCKET.init(DnsSocket::new(stack));
    let http_client = HTTP_CLIENT.init(HttpClient::new(tcp_client, dns_socket));

    (
        hardware,
        ConnectionControls {
            http: HttpConnection::Connected(http_client),
        },
    )
}
