// rusty_v8 137 is built with ICU 74 and keeps ICU symbols inside its static
// archive. Using ICU's public C API sets the same process-wide timezone used by
// native V8 Date and Intl. The link name must move with ICU when rusty_v8 is
// upgraded, so an incompatible V8 update fails at link time instead of quietly
// giving a mixed browser identity.
unsafe extern "C" {
    #[link_name = "ucal_setDefaultTimeZone_74"]
    fn ucal_set_default_time_zone(zone_id: *const u16, error_code: *mut i32);
}

/// Set the process-wide ICU timezone after V8 has loaded its ICU data.
pub(crate) fn set_default_timezone(timezone: &str) -> Result<(), String> {
    if timezone.is_empty() || timezone.contains('\0') {
        return Err("timezone must be a non-empty IANA name".to_string());
    }
    let zone: Vec<u16> = timezone.encode_utf16().chain(std::iter::once(0)).collect();
    let mut error_code = 0;
    // SAFETY: zone is NUL-terminated and remains alive for the call. ICU copies
    // the zone into its process-wide default. The runtime calls this under its
    // isolate creation lock before page script execution starts.
    unsafe {
        ucal_set_default_time_zone(zone.as_ptr(), &mut error_code);
    }
    if error_code > 0 {
        Err(format!(
            "ICU rejected timezone {timezone} with error {error_code}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_nul_timezone_names() {
        assert!(set_default_timezone("").is_err());
        assert!(set_default_timezone("Europe\0Moscow").is_err());
    }
}
