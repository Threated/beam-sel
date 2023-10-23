use std::{net::SocketAddr, convert::Infallible};

use beam_lib::{reqwest::Url, AppId};
use clap::Parser;
use std::net::ToSocketAddrs;

/// BeamSEL
#[derive(Debug, Parser)]
pub struct Config {
    /// Address the server should bind to
    #[clap(env, default_value = "0.0.0.0:8080")]
    pub bind_addr: SocketAddr,

    /// Url of the local beam proxy which is required to have sockets enabled
    #[clap(env, default_value = "http://beam-proxy:8081")]
    pub beam_url: Url,

    /// Beam api key
    #[clap(env)]
    pub beam_secret: String,

    /// The app id of this application
    #[clap(long, env, value_parser=|id: &str| Ok::<_, Infallible>(AppId::new_unchecked(id)))]
    pub beam_id: AppId,

    /// Address of the local SEL
    #[clap(env, default_value = "sel:8161", value_parser=|addr: &str| addr.to_socket_addrs())]
    pub sel_addr: SocketAddr,
}
