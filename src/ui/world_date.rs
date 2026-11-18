//! GuiSelectWorld uses Java 8's short SimpleDateFormat for the operating-system locale.
#[cfg(windows)]
pub(super) fn system_locale() -> String {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    }
    let mut name = [0; 85];
    let size = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    if size > 1 {
        String::from_utf16_lossy(&name[..size as usize - 1]).to_ascii_lowercase()
    } else {
        "en-us".to_owned()
    }
}
#[cfg(not(windows))]
pub(super) fn system_locale() -> String {
    std::env::var("LANG")
        .unwrap_or_default()
        .split('.')
        .next()
        .unwrap_or("en-US")
        .replace('_', "-")
        .to_lowercase()
}
#[cfg(windows)]
pub(super) fn format(millis: i64, locale: &str, korea_zone: bool) -> String {
    use std::{
        ffi::{c_char, c_void, CString},
        ptr,
    };
    #[link(name = "icu")]
    extern "C" {
        fn udat_open(
            time: i32,
            date: i32,
            locale: *const c_char,
            zone: *const u16,
            zone_len: i32,
            pattern: *const u16,
            len: i32,
            error: *mut i32,
        ) -> *mut c_void;
        fn udat_format(
            format: *const c_void,
            millis: f64,
            output: *mut u16,
            length: i32,
            pos: *mut c_void,
            error: *mut i32,
        ) -> i32;
        fn udat_close(format: *mut c_void);
    }
    let formats = include_str!("world_date_formats.tsv");
    let pattern = |key: &str| {
        formats
            .lines()
            .filter_map(|row| row.split_once('\t'))
            .find_map(|(names, pattern)| {
                names.split(',').any(|name| name == key).then_some(pattern)
            })
    };
    let format = pattern(locale)
        .or_else(|| pattern(locale.split('-').next().unwrap_or(locale)))
        .unwrap_or("M/d/yy h:mm a");
    let pattern: Vec<_> = format.encode_utf16().collect();
    let name = CString::new(locale).expect("Operating-system locale name");
    let tz: Vec<u16> = if korea_zone {
        "Asia/Seoul".encode_utf16().collect()
    } else {
        Vec::new()
    };
    let mut error = 0;
    let handle = unsafe {
        udat_open(
            -2,
            -2,
            name.as_ptr(),
            if tz.is_empty() {
                ptr::null()
            } else {
                tz.as_ptr()
            },
            tz.len() as i32,
            pattern.as_ptr(),
            pattern.len() as i32,
            &mut error,
        )
    };
    if handle.is_null() || error > 0 {
        return utc_date(millis);
    }
    let mut output = [0u16; 192];
    let size = unsafe {
        let n = udat_format(
            handle,
            millis as f64,
            output.as_mut_ptr(),
            output.len() as i32,
            ptr::null_mut(),
            &mut error,
        );
        udat_close(handle);
        n
    };
    if error <= 0 && (0..=output.len() as i32).contains(&size) {
        String::from_utf16_lossy(&output[..size as usize])
    } else {
        utc_date(millis)
    }
}
#[cfg(not(windows))]
pub(super) fn format(millis: i64, _locale: &str, _korea_zone: bool) -> String {
    utc_date(millis)
}
// Gregorian UTC fallback on platforms without the Windows ICU runtime.
fn utc_date(millis: i64) -> String {
    let minutes = millis.div_euclid(60_000);
    let day = minutes.div_euclid(1440) + 719468;
    let era = day.div_euclid(146097);
    let doe = day.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let m = (5 * doy + 2) / 153;
    let d = doy - (153 * m + 2) / 5 + 1;
    let month = if m < 10 { m + 3 } else { m - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{d:02} {:02}:{:02} UTC",
        minutes.div_euclid(60).rem_euclid(24),
        minutes.rem_euclid(60)
    )
}
