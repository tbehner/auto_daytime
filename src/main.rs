use structopt;
use std::path::PathBuf;
use structopt::StructOpt;
use shellexpand;
use anyhow::Result;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use chrono::prelude::*;
use neovim_lib::{Neovim, NeovimApi, Session};
use glob::glob;
use std::fs::OpenOptions;
use std::io::prelude::*;
use regex::Regex;

#[derive(Serialize, Deserialize, Debug)]
struct IPInfo {
    ip: String,
    hostname: String,
    city: String,
    region: String,
    country: String,
    loc: String,
    org: String,
    postal: String,
    timezone: String,
    readme: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct SunInfo {
    sunrise: String,
    sunset: String,
    solar_noon: String,
    day_length: String,
    civil_twilight_begin: String,
    civil_twilight_end: String,
    nautical_twilight_begin: String,
    nautical_twilight_end: String,
    astronomical_twilight_begin: String,
    astronomical_twilight_end: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct SunInfoResponse {
    results: SunInfo,
    status: String,
}

enum SunState {
    Up,
    Down
}



/// A basic example
#[derive(StructOpt, Debug)]
#[structopt(name = "basic")]
struct Opt {
    /// Set alacritty config path
    #[structopt(short, long, default_value = "~/.config/alacritty/alacritty.yml")]
    alacritty: PathBuf,

    /// Set nvim config path
    #[structopt(short, long, default_value = "~/.config/nvim/init.vim")]
    nvim_init: PathBuf,

    /// Force a light or dark mode
    #[structopt(short, long)]
    force: Option<String>,
}

fn get_local_dt(formatted_time: &str) -> Result<DateTime<Local>> {
    let naive_time = NaiveTime::parse_from_str(formatted_time, "%l:%M:%S %p")?;
    let today = Local::today().naive_local();
    let utc_time = Utc.from_utc_date(&today).and_time(naive_time).unwrap();
    let local_time = Local.from_utc_datetime(&utc_time.naive_local());
    Ok(local_time)
}

fn get_local_sun_state() -> Result<SunState> {
    let body = reqwest::blocking::get("https://ipinfo.io")?.text()?;
    let info: IPInfo = serde_json::from_str(&body)?;
    let coord: Vec<f64> = info.loc.split(",").map(|r| r.parse::<f64>().unwrap()).collect();

    let sun_info_body = reqwest::blocking::get(format!("https://api.sunrise-sunset.org/json?lat={}&lng={}", coord.get(0).unwrap(), coord.get(1).unwrap()))?.text()?;
    let sun_info: SunInfoResponse = serde_json::from_str(&sun_info_body)?;

    let local_sunrise = get_local_dt(&sun_info.results.sunrise)?;
    let local_sunset = get_local_dt(&sun_info.results.sunset)?;

    let state = if local_sunrise <= Local::now() && Local::now() < local_sunset {
        SunState::Up
    } else {
        SunState::Down
    };
    Ok(state)
}

fn set_running_nvim_sessions(state: &SunState) -> Result<()>{
    // connect to all neovim instances
    // set the correct background, reload AirlineTheme
    for nvim_path in glob("/tmp/nvim*/0")? {
        let nvim_socket = nvim_path.unwrap();
        let mut session = Session::new_unix_socket(&nvim_socket)?;
        session.start_event_loop();
        let mut nvim = Neovim::new(session);

        match state {
            SunState::Up => nvim.command("set bg=light")?,
            SunState::Down => nvim.command("set bg=dark")?,
        };

        
        let theme = nvim.command_output("AirlineTheme")?;
        nvim.command(&format!("AirlineTheme {}", theme))?;
    }

    Ok(())
}

fn set_static_nvim_config(state: &SunState) -> Result<()> {
    // create ~/.daylight.vim with default content if it does not exist
    let daylight_config: String = shellexpand::tilde("~/.daylight.vim").into();
    let daylight_config_path = PathBuf::from(daylight_config);

    if !daylight_config_path.is_file() {
        println!("You really need a ~/.daylight.vim for this to work!");
        return Ok(());
    }
    dbg!(&daylight_config_path);

    let mut daylight_file = OpenOptions::new()
        .append(false)
        .create(true)
        .write(true)
        .read(false)
        .open(&daylight_config_path)?;
    dbg!(&daylight_file);

    match state {
        SunState::Up => daylight_file.write_all(b"set bg=light\n")?,
        SunState::Down => daylight_file.write_all(b"set bg=dark\n")?,
    }

    Ok(())
}

fn set_static_alacritty_config(state: &SunState) -> Result<()> {
    let alacritty_config: String = shellexpand::tilde("~/.config/alacritty/alacritty.yml").into();
    let alacritty_config_path = PathBuf::from(&alacritty_config);

    let mut alacritty_config_file = OpenOptions::new()
        .read(true)
        .append(false)
        .write(true)
        .create(false)
        .open(&alacritty_config_path)?;

    let color_line = Regex::new(r"colors: \*(([_\w]+)light([_\w]+))")?;
    let mut config = String::new();
    alacritty_config_file.read_to_string(&mut config)?;
    let updated_config: Vec<String> = config.lines().map(|line|{
        if color_line.is_match(line) {
            line.replace(":", "...")
        } else {
            line.into()
        }
    }).collect();
    dbg!(updated_config);

    Ok(())
}

fn main() -> Result<()> {

    // let state = get_local_sun_state()?;
    let state = SunState::Up;
    // set_running_nvim_sessions(&state)?;
    // set_static_nvim_config(&state)?;
    set_static_alacritty_config(&state)?;

    // set the correct theme in the alacritty config
    Ok(())
}
