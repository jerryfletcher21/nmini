use std::io::{IsTerminal, Read};
use anyhow::{anyhow, Error};
use chrono::{DateTime, Local};

pub fn path_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

pub fn mkdir(path: &str) -> Result<(), Error> {
    std::fs::create_dir_all(std::path::PathBuf::from(&path))?;

    Ok(())
}

pub fn file_write(file_name: &str, content: &str) -> Result<(), Error> {
    Ok(std::fs::write(file_name, content)?)
}

pub fn json_to_string_pretty<T: ?Sized>(json_value: &T) -> String
where
    T: serde::ser::Serialize
{
    serde_json::to_string_pretty(&json_value)
        .unwrap_or_else(|_| format!("error: getting json string pretty"))
}

pub fn read_stdin_pipe() -> Result<String, Error> {
    let mut input = std::io::stdin();

    if input.is_terminal() {
        return Err(anyhow!("stdin is empty"));
    }

    let mut output = String::new();

    input.read_to_string(&mut output)?;

    Ok(output)
}

fn datetime_human_readable_format_get() -> &'static str {
    "%Y/%m/%d %H:%M:%S"
}

pub fn unix_timestamp_s_to_string(timestamp: u64) -> Result<String, Error> {
    let datetime =
        DateTime::from_timestamp(timestamp as i64, 0)
            .ok_or(anyhow!("datetime from timestamp seconds"))?
            .with_timezone(&Local);

    Ok(datetime.format(datetime_human_readable_format_get()).to_string())
}

//use chrono::{NaiveDateTime, TimeZone};
//pub fn unix_timestamp_s_from_string(date: &str) -> Result<u64, Error> {
//    Ok(Local
//        .from_local_datetime(
//            &NaiveDateTime::parse_from_str(date, datetime_human_readable_format_get())?
//        )
//        .single()
//        .ok_or(anyhow!("converting date to timestamp"))?
//        .timestamp()
//    as u64)
//}

pub fn u64_from_serde_value(
    object: &serde_json::Value, key: &str
) -> Result<u64, Error> {
    Ok(object.get(key)
        .ok_or(anyhow!("{key} not present"))?
        .as_number()
        .ok_or(anyhow!("{key} not number"))?
        .as_u64()
        .ok_or(anyhow!("{key} not u64"))?
    )
}
