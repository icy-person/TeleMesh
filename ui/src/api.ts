import type {Dialog,Event,Health,Me,Message} from "./types";
export class TeleMeshApi{
 constructor(public base:string,public token:string){}
 private async req<T>(path:string,init:RequestInit={}):Promise<T>{
  const r=await fetch(this.base.replace(/\/$/,"")+path,{...init,headers:{"content-type":"application/json","x-telemesh-token":this.token,...(init.headers||{})}});
  if(!r.ok){
   const body=await r.text();
   try{const j=JSON.parse(body);throw new Error(j.error||body||r.statusText)}catch(e){if(e instanceof Error)throw e;throw new Error(body||r.statusText)}
  }
  return r.json();
 }
 health(){return this.req<Health>("/health")}
 me(){return this.req<Me>("/api/v1/me")}
 dialogs(){return this.req<Dialog[]>("/api/v1/dialogs")}
 messages(peer:string,limit=50,offsetId=0){return this.req<{messages:Message[];has_more:boolean}>(`/api/v1/messages?peer=${encodeURIComponent(peer)}&limit=${limit}&offset_id=${offsetId}`)}
 send(peer:string,text:string){return this.req<{message_id:number;peer_id:number}>("/api/v1/messages/send",{method:"POST",body:JSON.stringify({peer,text})})}
 loginStart(phone:string){return this.req<{status:string;requires_code:boolean}>("/api/v1/auth/start",{method:"POST",body:JSON.stringify({phone})})}
 loginComplete(code:string){return this.req<Me>("/api/v1/auth/complete",{method:"POST",body:JSON.stringify({code})})}
 loginPassword(password:string){return this.req<Me>("/api/v1/auth/password",{method:"POST",body:JSON.stringify({password})})}
 async ticket(){return this.req<{ticket:string}>("/api/v1/events/ticket",{method:"POST"})}
 async events(onEvent:(e:Event)=>void,onState:(s:boolean)=>void){
  const {ticket}=await this.ticket();
  const u=new URL(this.base.replace(/^http/,"ws")+"/api/v1/events"); u.searchParams.set("ticket",ticket);
  const ws=new WebSocket(u); ws.onopen=()=>onState(true); ws.onclose=()=>onState(false); ws.onerror=()=>onState(false);
  ws.onmessage=e=>{try{onEvent(JSON.parse(e.data))}catch{}}; return ()=>ws.close();
 }
}
export function messageText(m:Message){return m.text||"(media)"}
