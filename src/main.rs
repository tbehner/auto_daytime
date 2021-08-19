use structopt;
use std::path::PathBuf;
use clap::arg_enum;
use structopt::StructOpt;
use shellexpand;
use anyhow::Result;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use chrono::prelude::*;
use neovim_lib::{Neovim, NeovimApi, Session};
use glob::glob;
use std::fs::{remove_file,OpenOptions, File};
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

arg_enum! {
    #[derive(Debug,PartialEq)]
    enum SunState {
        Up,
        Down
    }
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
    #[structopt(short, long, possible_values = &SunState::variants(), case_insensitive = true)]
    force: Option<SunState>,
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

fn get_daylight_config() -> Result<PathBuf> {
    let daylight_config: String = shellexpand::tilde("~/.daylight.vim").into();
    let daylight_config_path = PathBuf::from(daylight_config);

    if !daylight_config_path.is_file() {
        println!("You really need a ~/.daylight.vim for this to work!");
        let mut daylight_config_file = OpenOptions::new().create(true).write(true).open(&daylight_config_path)?;
        daylight_config_file.write_all("set bg=light\n".as_bytes());
    }

    Ok(daylight_config_path)
}

fn get_static_daylight() -> Result<SunState> {
    let daylight_config = get_daylight_config()?;
    let mut daylight_file = File::open(&daylight_config)?;
    let mut daylight_content = String::new();
    daylight_file.read_to_string(&mut daylight_content)?;

    let up_pattern = Regex::new("dark")?;
    if up_pattern.is_match(&daylight_content){
        Ok(SunState::Down)
    } else {
        Ok(SunState::Up)
    }
}

fn set_static_nvim_config(state: &SunState) -> Result<()> {
    let daylight_config_path = get_daylight_config()?;

    let mut daylight_file = OpenOptions::new()
        .append(false)
        .create(true)
        .write(true)
        .read(false)
        .open(&daylight_config_path)?;

    match state {
        SunState::Up => daylight_file.write_all(b"set bg=light\n")?,
        SunState::Down => daylight_file.write_all(b"set bg=dark\n")?,
    }

    Ok(())
}

fn set_static_alacritty_config(state: &SunState) -> Result<()> {
    let alacritty_config: String = shellexpand::tilde("~/.config/alacritty/alacritty.yml").into();
    let alacritty_config_path = PathBuf::from(&alacritty_config);
    
    if ! alacritty_config_path.is_file()  {
        return Err(anyhow!("Alacritty config missing."));
    }

    let mut config = String::new();
    let mut alacritty_config_file = OpenOptions::new()
        .read(true)
        .append(false)
        .write(true)
        .create(false)
        .open(&alacritty_config_path)?;

    alacritty_config_file.read_to_string(&mut config)?;

    let color_line = Regex::new(r"colors: \*(([_\w]+)(light|dark)([_\w]+))")?;

    let state_string: String = match state {
        SunState::Up => "light".into(),
        SunState::Down => "dark".into(),
    };

    let updated_config: Vec<String> = config.lines().map(|line|{
        match color_line.captures(line) {
            Some(caps) => {
                format!("colors: *{}{}{}", caps.get(2).unwrap().as_str(), &state_string, caps.get(4).unwrap().as_str())
            },
            None => line.into()
        }
    }).collect();

    remove_file(&alacritty_config_path)?;
    let mut new_alacritty_config_file = File::create(&alacritty_config_path)?;
    new_alacritty_config_file.write_all(format!("{}\n", updated_config.join("\n")).as_bytes())?;

    Ok(())
}

fn main() -> Result<()> {
    let opt = Opt::from_args();

    let state = match opt.force {
        Some(s) => s,
        None => get_local_sun_state()?,
    };

    let set_state = get_static_daylight()?;
    if state != set_state {
        set_running_nvim_sessions(&state)?;
        set_static_nvim_config(&state)?;
        set_static_alacritty_config(&state)?;
    }

    Ok(())
}
