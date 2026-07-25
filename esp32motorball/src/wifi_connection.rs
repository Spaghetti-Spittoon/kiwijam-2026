use crate::hardware_initialisation::{HardwareControls, WifiState};
use esp_println::println;
use esp_radio::wifi::{
    sta::StationConfig, AuthenticationMethod, Config, ControllerConfig, WifiController,
};

const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASSWORD");

static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static TCP_STATE: StaticCell<TcpClientState<1, 1500, 1500>> = StaticCell::new();

pub enum TcpConnection {
    NotConnected,
    Connected(TcpClient<'static, 1, 1500, 1500>),
}

pub enum DnsConnection {
    NotConnected,
    Connected(DnsClient<'static, 1, 1500>),
}

pub enum HttpConnection {
    NotConnected,
    Connected(HttpClient<'static, 1, 1500>),
}

pub struct ConnectionControls {
    pub tcp: TcpConnection,
    pub dns: DnsConnection,
    pub http: HttpConnection,
}

pub async fn connect_wifi(mut hardware: HardwareControls) -> (HardwareControls, ConnectionControls) {
    let wifi_peripheral = match hardware.wifi {
        WifiState::NotConnected(p) => p,
        WifiState::Connected(_) => return (hardware, ConnectionControls {
            tcp: TcpConnection::NotConnected,
            dns: DnsConnection::NotConnected,
            http: HttpConnection::NotConnected,
        }),
    };

    let station_config = Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into())
            .with_auth_method(AuthenticationMethod::Wpa2Personal),
    );

    println!("Starting WiFi");

    let mut controller = WifiController::new(
        wifi_peripheral,
        ControllerConfig::default().with_initial_config(station_config),
    )
    .unwrap();

    println!("Wifi configured and started!");

    match controller.connect_async().await {
        Ok(info) => println!("Wifi connected to {:?}", info),
        Err(e) => {
            println!("Failed to connect to wifi: {e:?}");
            hardware.wifi = WifiState::NotConnected(controller.into_peripheral());
            return (hardware, ConnectionControls {
                tcp: TcpConnection::NotConnected,
                dns: DnsConnection::NotConnected,
                http: HttpConnection::NotConnected,
            });
        },
    }

    println!("starting network stack");
    let config = embassy_net::Config::dhcpv4_default();
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let resources = STACK_RESOURCES.init(StackResources::new());

    let (stack, runner) = embassy_net::Stack::new(
        controller,
        config,
        resources,
        seed,
    );
    spawner.spawn(connection(controller)).unwrap();
    spawner.spawn(net_task(runner)).unwrap();

    println!("waiting for network stack");
    stack.wait_config_up().await;
    println!("network stack is up");

    let tcp_client = TcpClient::new(stack, TCP_STATE.init(TcpClientState::new()));
    let dns_client = DnsSocket::new(stack);
    let http_client = HttpClient::new(&tcp_client, &dns_client);

    hardware.wifi = WifiState::Connected(controller);

    return (hardware, ConnectionControls {
        tcp: TcpConnection::Connected(tcp_client),
        dns: DnsConnection::Connected(dns_client),
        http: HttpConnection::Connected(http_client),
    });
}
