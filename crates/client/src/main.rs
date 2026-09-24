use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use reqwest::Client;
use telemesh_protocol::{DialogDto,HealthResponse,MeResponse,SendMessageRequest};

#[derive(Parser)]
#[command(name="telemesh")]
struct Cli{
 #[arg(long,env="TELEMESH_SERVER",default_value="http://127.0.0.1:8787")] server:String,
 #[arg(long,env="TELEMESH_CLIENT_TOKEN")] token:String,
 #[command(subcommand)] command:Command
}
#[derive(Subcommand)]
enum Command{Health,Me,Dialogs,Send{peer:String,text:String},Events{#[arg(long)] ws:Option<String>}}

async fn get<T:serde::de::DeserializeOwned>(c:&Cli,path:&str)->Result<T>{
 Ok(Client::new().get(format!("{}{}",c.server.trim_end_matches('/'),path)).header("x-telemesh-token",&c.token).send().await?.error_for_status()?.json().await?)
}
#[tokio::main]
async fn main()->Result<()>{
 let c=Cli::parse();
 match &c.command{
  Command::Health=>println!("{}",serde_json::to_string_pretty(&get::<HealthResponse>(&c,"/health").await?)?),
  Command::Me=>println!("{}",serde_json::to_string_pretty(&get::<MeResponse>(&c,"/api/v1/me").await?)?),
  Command::Dialogs=>{let v:Vec<DialogDto>=get(&c,"/api/v1/dialogs").await?;println!("{}",serde_json::to_string_pretty(&v)?);},
  Command::Send{peer,text}=>{
   let v=Client::new().post(format!("{}/api/v1/messages/send",c.server.trim_end_matches('/'))).header("x-telemesh-token",&c.token).json(&SendMessageRequest{peer:peer.clone(),text:text.clone()}).send().await?.error_for_status()?.json::<telemesh_protocol::SendMessageResponse>().await?;
   println!("{}",serde_json::to_string_pretty(&v)?);
  },
  Command::Events{ws}=>{
   let url=ws.clone().unwrap_or_else(||format!("{}/api/v1/events",c.server.trim_end_matches('/').replace("http://","ws://").replace("https://","wss://")));
   let (mut stream,_)=tokio_tungstenite::connect_async(url).await?;
   while let Some(m)=stream.next().await{println!("{:?}",m?);}
  }
 }
 Ok(())
}
