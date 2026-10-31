//! Server List Ping: online count and MOTD for the launcher's header.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub online: u32,
    pub max: u32,
    pub version: String,
}

fn varint(mut v: i32, out: &mut Vec<u8>) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

async fn read_varint(s: &mut TcpStream) -> Result<i32> {
    let mut v = 0i32;
    for i in 0..5 {
        let b = s.read_u8().await?;
        v |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    anyhow::bail!("varint too long")
}

pub async fn ping(host: &str, port: u16) -> Result<Status> {
    tokio::time::timeout(Duration::from_secs(5), ping_inner(host, port))
        .await
        .context("timeout")?
}

async fn ping_inner(host: &str, port: u16) -> Result<Status> {
    let mut s = TcpStream::connect((host, port)).await?;
    let mut hs = Vec::new();
    varint(0, &mut hs); // handshake packet id
    varint(-1, &mut hs); // protocol: any
    varint(host.len() as i32, &mut hs);
    hs.extend_from_slice(host.as_bytes());
    hs.extend_from_slice(&port.to_be_bytes());
    varint(1, &mut hs); // next state: status
    let mut pkt = Vec::new();
    varint(hs.len() as i32, &mut pkt);
    pkt.extend(hs);
    pkt.extend_from_slice(&[1, 0]); // status request
    s.write_all(&pkt).await?;

    let _len = read_varint(&mut s).await?;
    let _id = read_varint(&mut s).await?;
    let n = read_varint(&mut s).await? as usize;
    anyhow::ensure!(n < 1 << 20, "status too large");
    let mut buf = vec![0; n];
    s.read_exact(&mut buf).await?;

    #[derive(Deserialize)]
    struct Raw {
        players: Players,
        version: Version,
    }
    #[derive(Deserialize)]
    struct Players {
        online: u32,
        max: u32,
    }
    #[derive(Deserialize)]
    struct Version {
        name: String,
    }
    let raw: Raw = serde_json::from_slice(&buf)?;
    Ok(Status {
        online: raw.players.online,
        max: raw.players.max,
        version: raw.version.name,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn varint() {
        let mut v = Vec::new();
        super::varint(300, &mut v);
        assert_eq!(v, [0xac, 0x02]);
        v.clear();
        super::varint(-1, &mut v);
        assert_eq!(v, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    }
}
