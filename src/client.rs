use std::fs;
use std::path::Path;
use std::time::Duration;
use async_std::io;
use log::{info};
use serde::{Deserialize, Serialize};
use crate::{RpcMessage, RpcValue};
use crate::connection::FrameReader;
use crate::util::sha1_password_hash;

#[derive(Copy, Clone, Debug)]
pub enum LoginType {
    PLAIN,
    SHA1,
}
impl LoginType {
    pub fn to_str(&self) -> &str {
        match self {
            LoginType::PLAIN => "PLAIN",
            LoginType::SHA1 => "SHA1",
        }
    }
}

pub enum Scheme {
    Tcp,
    LocalSocket,
}

#[derive(Clone, Debug)]
pub struct LoginParams {
    pub user: String,
    pub password: String,
    pub login_type: LoginType,
    pub device_id: String,
    pub mount_point: String,
    pub heartbeat_interval: Option<Duration>,
    //pub protocol: Protocol,
}

impl Default for LoginParams {
    fn default() -> Self {
        LoginParams {
            user: "".to_string(),
            password: "".to_string(),
            login_type: LoginType::SHA1,
            device_id: "".to_string(),
            mount_point: "".to_string(),
            heartbeat_interval: Some(Duration::from_secs(60)),
            //protocol: Protocol::ChainPack,
        }
    }
}

impl LoginParams {
    pub fn to_rpcvalue(&self) -> RpcValue {
        let mut map = crate::Map::new();
        let mut login = crate::Map::new();
        login.insert("user".into(), RpcValue::from(&self.user));
        login.insert("password".into(), RpcValue::from(&self.password));
        login.insert("type".into(), RpcValue::from(self.login_type.to_str()));
        map.insert("login".into(), RpcValue::from(login));
        let mut options = crate::Map::new();
        if let Some(hbi) = self.heartbeat_interval {
            options.insert(
                "idleWatchDogTimeOut".into(),
                RpcValue::from(hbi.as_secs() * 3),
            );
        }
        let mut device = crate::Map::new();
        if !self.device_id.is_empty() {
            device.insert("deviceId".into(), RpcValue::from(&self.device_id));
        } else if !self.mount_point.is_empty() {
            device.insert("mountPoint".into(), RpcValue::from(&self.mount_point));
        }
        if !device.is_empty() {
            options.insert("device".into(), RpcValue::from(device));
        }
        map.insert("options".into(), RpcValue::from(options));
        RpcValue::from(map)
    }
}

pub async fn login<'a, R, W>(frame_reader: &mut FrameReader<'a, R>, writer: &mut W, login_params: &LoginParams) -> crate::Result<i32>
where R: io::Read + std::marker::Unpin,
      W: io::Write + std::marker::Unpin
{
    let rq = RpcMessage::new_request("", "hello", None);
    crate::connection::send_message(writer, &rq).await?;
    let resp = frame_reader.receive_message().await?.unwrap_or_default();
    if !resp.is_success() {
        return Err(resp.error().unwrap().to_rpcvalue().to_cpon().into());
    }
    let nonce = resp.result()?.as_map()
        .get("nonce").ok_or("Bad nonce")?.as_str();
    let hash = sha1_password_hash(login_params.password.as_bytes(), nonce.as_bytes());
    let mut login_params = login_params.clone();
    login_params.password = std::str::from_utf8(&hash)?.into();
    let rq = RpcMessage::new_request("", "login", Some(login_params.to_rpcvalue()));
    crate::connection::send_message(writer, &rq).await?;
    let resp = frame_reader.receive_message().await?.ok_or("Socked closed")?;
    match resp.result()?.as_map().get("clientId") {
        None => { Ok(0) }
        Some(client_id) => { Ok(client_id.as_i32()) }
    }
}
fn default_heartbeat() -> String { "1m".into() }
#[derive(Serialize, Deserialize, Debug)]
pub struct ClientConfig {
    pub url: String,
    pub device_id: Option<String>,
    pub mount: Option<String>,
    #[serde(default = "default_heartbeat")]
    pub heartbeat_interval: String,
    pub reconnect_interval: Option<String>,
}
impl ClientConfig {
    pub fn from_file(file_name: &str) -> crate::Result<Self> {
        let content = fs::read_to_string(file_name)?;
        Ok(serde_yaml::from_str(&content)?)
    }
    pub fn from_file_or_default(file_name: &str, create_if_not_exist: bool) -> crate::Result<Self> {
        let file_path = Path::new(file_name);
        if file_path.exists() {
            info!("Loading config file {file_name}");
            return match Self::from_file(&file_name) {
                Ok(cfg) => {
                    Ok(cfg)
                }
                Err(err) => {
                    Err(format!("Cannot read config file: {file_name} - {err}").into())
                }
            }
        }
        let config = Default::default();
        if create_if_not_exist {
            if let Some(config_dir) = file_path.parent() {
                fs::create_dir_all(config_dir)?;
            }
            info!("Creating default config file: {file_name}");
            fs::write(file_path, serde_yaml::to_string(&config)?)?;
        }
        Ok(config)
    }
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            url: "tcp://localhost:3755".to_string(),
            device_id: None,
            mount: None,
            heartbeat_interval: default_heartbeat(),
            reconnect_interval: None,
        }
    }
}