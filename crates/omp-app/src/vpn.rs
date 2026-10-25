//! The built-in VPN (Gluetun, started by the container's entrypoint when
//! `VPN_SERVICE_PROVIDER` is set). The app only reads its status from
//! Gluetun's local control server; it never changes the tunnel.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct VpnStatus {
    /// Gluetun is part of this container.
    pub enabled: bool,
    pub provider: Option<String>,
    /// "running", "stopped", ... or "unreachable" when Gluetun does not answer.
    pub status: Option<String>,
    pub public_ip: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
}

fn control_url() -> Option<String> {
    std::env::var("OMP_GLUETUN_URL")
        .ok()
        .map(|u| u.trim_end_matches('/').to_owned())
        .filter(|u| !u.is_empty())
}

pub async fn status() -> VpnStatus {
    match control_url() {
        Some(url) => status_from(&url, std::env::var("VPN_SERVICE_PROVIDER").ok()).await,
        None => VpnStatus::default(),
    }
}

pub async fn status_from(url: &str, provider: Option<String>) -> VpnStatus {
    let mut out = VpnStatus {
        enabled: true,
        provider: provider.filter(|p| !p.is_empty()),
        status: Some("unreachable".into()),
        ..Default::default()
    };
    // Loopback only: never through the configured proxy.
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return out;
    };
    let get = |path: &str| {
        let client = client.clone();
        let url = format!("{url}{path}");
        async move {
            let r = client.get(url).send().await.ok()?;
            r.status().is_success().then_some(())?;
            r.json::<Value>().await.ok()
        }
    };
    let Some(v) = get("/v1/vpn/status").await else {
        return out;
    };
    out.status = v["status"].as_str().map(str::to_owned);
    if let Some(ip) = get("/v1/publicip/ip").await {
        let text = |k: &str| ip[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        out.public_ip = text("public_ip");
        out.country = text("country");
        out.region = text("region");
        out.city = text("city");
    }
    out
}

/// The VPN set up from the Settings page. It is saved as `vpn.env` in the
/// data folder, which the container's entrypoint reads before it starts
/// Gluetun (so a change applies after a restart). Settings from the
/// container's environment win, and then the page cannot change them.
pub mod settings {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};

    const FILE: &str = "vpn.env";

    /// The VPN providers Gluetun knows, as it spells them.
    pub const PROVIDERS: &[&str] = &[
        "airvpn",
        "custom",
        "cyberghost",
        "expressvpn",
        "fastestvpn",
        "giganews",
        "hidemyass",
        "ipvanish",
        "ivpn",
        "mullvad",
        "nordvpn",
        "perfect privacy",
        "privado",
        "private internet access",
        "privatevpn",
        "protonvpn",
        "purevpn",
        "slickvpn",
        "surfshark",
        "torguard",
        "vpn secure",
        "vpn unlimited",
        "vyprvpn",
        "windscribe",
    ];

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    pub struct VpnSettings {
        /// Empty means no VPN.
        pub provider: String,
        /// "openvpn" or "wireguard".
        pub vpn_type: String,
        pub username: String,
        /// Not sent back; an empty one on save keeps the saved one.
        pub password: String,
        pub wireguard_private_key: String,
        pub wireguard_addresses: String,
        pub server_countries: String,
        pub server_cities: String,
        pub server_hostnames: String,
        /// Local networks the app may reach, such as 192.168.1.0/24.
        pub firewall_outbound_subnets: String,
    }

    type Getter = fn(&VpnSettings) -> &String;
    type Setter = fn(&mut VpnSettings) -> &mut String;
    const KEYS: [(&str, Getter, Setter); 10] = [
        ("VPN_SERVICE_PROVIDER", |s| &s.provider, |s| &mut s.provider),
        ("VPN_TYPE", |s| &s.vpn_type, |s| &mut s.vpn_type),
        ("OPENVPN_USER", |s| &s.username, |s| &mut s.username),
        ("OPENVPN_PASSWORD", |s| &s.password, |s| &mut s.password),
        (
            "WIREGUARD_PRIVATE_KEY",
            |s| &s.wireguard_private_key,
            |s| &mut s.wireguard_private_key,
        ),
        (
            "WIREGUARD_ADDRESSES",
            |s| &s.wireguard_addresses,
            |s| &mut s.wireguard_addresses,
        ),
        (
            "SERVER_COUNTRIES",
            |s| &s.server_countries,
            |s| &mut s.server_countries,
        ),
        (
            "SERVER_CITIES",
            |s| &s.server_cities,
            |s| &mut s.server_cities,
        ),
        (
            "SERVER_HOSTNAMES",
            |s| &s.server_hostnames,
            |s| &mut s.server_hostnames,
        ),
        (
            "FIREWALL_OUTBOUND_SUBNETS",
            |s| &s.firewall_outbound_subnets,
            |s| &mut s.firewall_outbound_subnets,
        ),
    ];

    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(FILE)
    }

    /// The VPN is set by the container's environment, not by this page.
    pub fn from_environment() -> bool {
        std::env::var("VPN_SERVICE_PROVIDER").is_ok_and(|p| !p.is_empty())
            && std::env::var("OMP_VPN_FROM_FILE").is_err()
    }

    pub fn load(data_dir: &Path) -> VpnSettings {
        let mut out = VpnSettings::default();
        let Ok(text) = std::fs::read_to_string(path(data_dir)) else {
            return out;
        };
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=')
                && let Some((_, _, set)) = KEYS.iter().find(|(name, _, _)| *name == k)
            {
                *set(&mut out) = v.to_owned();
            }
        }
        out
    }

    /// Checks what a person typed. Values go to a file read line by line, so
    /// they may not hold a line break, and the names are Gluetun's.
    pub fn check(s: &VpnSettings) -> Result<(), String> {
        for (name, get, _) in KEYS {
            let v = get(s);
            if v.contains(['\n', '\r', '\0']) {
                return Err(format!("{name} may not contain a line break"));
            }
        }
        if !s.provider.is_empty() && !PROVIDERS.contains(&s.provider.as_str()) {
            return Err(format!(
                "'{}' is not a VPN provider Gluetun supports",
                s.provider
            ));
        }
        if !matches!(s.vpn_type.as_str(), "" | "openvpn" | "wireguard") {
            return Err("the connection type must be OpenVPN or WireGuard".into());
        }
        let subnets = s.firewall_outbound_subnets.trim();
        if !subnets.is_empty()
            && !subnets.split(',').all(|n| {
                let n = n.trim();
                n.split_once('/').is_some_and(|(ip, bits)| {
                    ip.parse::<std::net::IpAddr>().is_ok()
                        && bits.parse::<u8>().is_ok_and(|b| b <= 128)
                })
            })
        {
            return Err("local networks look like 192.168.1.0/24, separated by commas".into());
        }
        if !s.provider.is_empty()
            && s.vpn_type == "wireguard"
            && s.wireguard_private_key.trim().is_empty()
        {
            return Err("WireGuard needs a private key".into());
        }
        Ok(())
    }

    /// Saves the settings; an empty provider turns the VPN off. A blank
    /// password or key keeps the one already saved.
    pub fn save(data_dir: &Path, mut new: VpnSettings) -> Result<(), String> {
        let old = load(data_dir);
        if new.password.is_empty() {
            new.password = old.password;
        }
        if new.wireguard_private_key.is_empty() {
            new.wireguard_private_key = old.wireguard_private_key;
        }
        check(&new)?;
        let file = path(data_dir);
        if new.provider.is_empty() {
            return match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        let mut text = String::new();
        for (name, get, _) in KEYS {
            let v = get(&new).trim();
            if !v.is_empty() {
                text.push_str(&format!("{name}={v}\n"));
            }
        }
        write_private(&file, &text).map_err(|e| e.to_string())
    }

    /// Written to a temporary file with the owner-only mode first, so the
    /// secrets are never readable by others, then moved into place.
    fn write_private(file: &Path, text: &str) -> std::io::Result<()> {
        use std::io::Write;
        let tmp = file.with_extension("env.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(tmp, file)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn dir() -> PathBuf {
            let d = std::env::temp_dir()
                .join(format!("omp-vpn-{}", std::process::id()))
                .join(format!("{:?}", std::thread::current().id()).replace(['(', ')'], ""));
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        fn mullvad() -> VpnSettings {
            VpnSettings {
                provider: "mullvad".into(),
                vpn_type: "wireguard".into(),
                wireguard_private_key: "KEY".into(),
                wireguard_addresses: "10.64.0.2/32".into(),
                server_countries: "Sweden".into(),
                ..Default::default()
            }
        }

        #[test]
        fn settings_are_saved_for_the_entrypoint_and_read_back() {
            let d = dir();
            save(&d, mullvad()).unwrap();
            let text = std::fs::read_to_string(path(&d)).unwrap();
            assert!(text.contains("VPN_SERVICE_PROVIDER=mullvad\n"));
            assert!(text.contains("WIREGUARD_PRIVATE_KEY=KEY\n"));
            assert_eq!(load(&d), mullvad());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(path(&d)).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }

        #[test]
        fn a_blank_secret_keeps_the_saved_one_and_no_provider_turns_the_vpn_off() {
            let d = dir();
            save(&d, mullvad()).unwrap();
            let mut again = mullvad();
            again.wireguard_private_key.clear();
            again.server_countries = "Norway".into();
            save(&d, again).unwrap();
            let now = load(&d);
            assert_eq!(now.wireguard_private_key, "KEY");
            assert_eq!(now.server_countries, "Norway");
            save(&d, VpnSettings::default()).unwrap();
            assert!(!path(&d).exists());
        }

        #[test]
        fn nonsense_is_refused() {
            let mut s = mullvad();
            s.provider = "nordvpn\nFOO=bar".into();
            assert!(check(&s).is_err());
            let mut s = mullvad();
            s.provider = "notavpn".into();
            assert!(check(&s).is_err());
            let mut s = mullvad();
            s.wireguard_private_key.clear();
            assert!(check(&s).is_err());
            let mut s = mullvad();
            s.firewall_outbound_subnets = "192.168.1.0/24, 10.0.0.0/8".into();
            assert!(check(&s).is_ok());
            s.firewall_outbound_subnets = "my lan".into();
            assert!(check(&s).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    use super::*;

    /// Built here so the OpenAPI scan, which reads route literals, does not mistake Gluetun's routes for ours.
    fn gluetun(path: &str) -> String {
        format!("/v1/{path}")
    }

    async fn serve(app: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        url
    }

    #[tokio::test]
    async fn reads_status_and_address_from_gluetun() {
        let url = serve(
            Router::new()
                .route(&gluetun("vpn/status"), get(|| async { Json(json!({"status": "running"})) }))
                .route(
                    &gluetun("publicip/ip"),
                    get(|| async {
                        Json(json!({"public_ip": "193.32.127.7", "country": "Sweden", "region": "Stockholm", "city": ""}))
                    }),
                ),
        )
        .await;
        let s = status_from(&url, Some("mullvad".into())).await;
        assert_eq!(s.status.as_deref(), Some("running"));
        assert_eq!(s.public_ip.as_deref(), Some("193.32.127.7"));
        assert_eq!(s.country.as_deref(), Some("Sweden"));
        assert_eq!(s.city, None, "an empty field is not a value");
        assert_eq!(s.provider.as_deref(), Some("mullvad"));
    }

    #[tokio::test]
    async fn a_silent_gluetun_is_unreachable_not_an_error() {
        let s = status_from("http://127.0.0.1:1", None).await;
        assert!(s.enabled);
        assert_eq!(s.status.as_deref(), Some("unreachable"));
    }

    #[tokio::test]
    async fn without_gluetun_the_vpn_is_off() {
        assert!(!VpnStatus::default().enabled);
    }
}
