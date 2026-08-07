use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::header::ACCEPT;
use serde::Deserialize;

const DATABASE_NAME: &str = "geoip.mmdb";
const EXIT_IP_URL: &str = "http://ifconfig.io/ip";
const EXIT_IP_LIMIT: u64 = 128;

#[derive(Debug)]
pub struct GeoIdentity {
    pub database: PathBuf,
    pub ip: IpAddr,
    pub country_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
}

#[derive(Deserialize)]
struct GeoRecord {
    country: GeoCountry,
    location: GeoLocation,
}

#[derive(Deserialize)]
struct GeoCountry {
    iso_code: String,
}

#[derive(Deserialize)]
struct GeoLocation {
    latitude: f64,
    longitude: f64,
    time_zone: String,
}

pub fn resolve(proxy: &str, explicit_database: Option<&Path>) -> Result<Option<GeoIdentity>> {
    let Some(database) = find_database(explicit_database)? else {
        return Ok(None);
    };
    let ip = resolve_exit_ip(proxy)?;
    let record = lookup(&database, ip)?;
    validate_record(&record)?;
    Ok(Some(GeoIdentity {
        database,
        ip,
        country_code: record.country.iso_code,
        latitude: record.location.latitude,
        longitude: record.location.longitude,
        timezone: record.location.time_zone,
    }))
}

fn find_database(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        if !path.is_file() {
            bail!("GeoIP database does not exist: {}", path.display());
        }
        return Ok(Some(path.to_path_buf()));
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            let path = directory.join(DATABASE_NAME);
            if path.is_file() {
                return Ok(Some(path));
            }
        }
    }

    let path = PathBuf::from(DATABASE_NAME);
    Ok(path.is_file().then_some(path))
}

fn resolve_exit_ip(proxy: &str) -> Result<IpAddr> {
    let proxy = reqwest::Proxy::all(proxy).context("invalid proxy URL for GeoIP lookup")?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build the GeoIP IP client")?;
    let response = client
        .get(EXIT_IP_URL)
        .header(ACCEPT, "text/plain")
        .send()
        .context("failed to get the proxy exit IP from ifconfig.io")?
        .error_for_status()
        .context("ifconfig.io rejected the proxy exit IP request")?;
    parse_exit_ip(response)
}

fn parse_exit_ip(mut input: impl Read) -> Result<IpAddr> {
    let mut body = String::new();
    input
        .by_ref()
        .take(EXIT_IP_LIMIT + 1)
        .read_to_string(&mut body)
        .context("failed to read the proxy exit IP")?;
    if body.len() as u64 > EXIT_IP_LIMIT {
        bail!("ifconfig.io returned more than {EXIT_IP_LIMIT} bytes");
    }
    body.trim()
        .parse()
        .context("ifconfig.io did not return a valid IP address")
}

fn lookup(database: &Path, ip: IpAddr) -> Result<GeoRecord> {
    let reader = maxminddb::Reader::open_readfile(database)
        .with_context(|| format!("failed to open GeoIP database {}", database.display()))?;
    let value = reader
        .lookup(ip)?
        .decode::<serde_json::Value>()?
        .with_context(|| format!("GeoIP database has no record for {ip}"))?;
    serde_json::from_value(value).context("GeoIP record has an invalid shape")
}

fn validate_record(record: &GeoRecord) -> Result<()> {
    if record.country.iso_code.trim().is_empty() {
        bail!("GeoIP record has an empty country code");
    }
    if record.location.time_zone.trim().is_empty() {
        bail!("GeoIP record has an empty timezone");
    }
    if !record.location.latitude.is_finite() || !(-90.0..=90.0).contains(&record.location.latitude) {
        bail!("GeoIP record has an invalid latitude");
    }
    if !record.location.longitude.is_finite() || !(-180.0..=180.0).contains(&record.location.longitude) {
        bail!("GeoIP record has an invalid longitude");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_ipv4_and_ipv6_exit_addresses() {
        assert_eq!(
            parse_exit_ip(Cursor::new("203.0.113.7\n")).unwrap(),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            parse_exit_ip(Cursor::new("  2001:db8::7  ")).unwrap(),
            "2001:db8::7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn rejects_non_ip_and_large_exit_responses() {
        assert!(parse_exit_ip(Cursor::new("not an ip")).is_err());
        assert!(parse_exit_ip(Cursor::new("x".repeat(129))).is_err());
    }

    #[test]
    fn reads_and_validates_nested_geoip_record_shape() {
        let record: GeoRecord = serde_json::from_value(serde_json::json!({
            "country": { "iso_code": "RU" },
            "location": {
                "latitude": 55.7558,
                "longitude": 37.6173,
                "time_zone": "Europe/Moscow"
            }
        }))
        .unwrap();
        validate_record(&record).unwrap();
        assert_eq!(record.country.iso_code, "RU");
        assert_eq!(record.location.time_zone, "Europe/Moscow");
    }

    #[test]
    fn rejects_invalid_nested_coordinates() {
        let record = GeoRecord {
            country: GeoCountry {
                iso_code: "RU".to_string(),
            },
            location: GeoLocation {
                latitude: 91.0,
                longitude: 37.6173,
                time_zone: "Europe/Moscow".to_string(),
            },
        };
        assert!(validate_record(&record).is_err());
    }
}
