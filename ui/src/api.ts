import type {Dialog,Event,Health,Me,Message} from "./types";
export class TeleMeshApi{
 constructor(public base:string,public token:string){}
 private async req<T>(path:string,init:RequestInit={}):Promise<T>{
  const r=await fetch(this.base.replace(/\/$/,"")+path,{...init,headers:{...(init.body instanceof FormData?{}:{"content-type":"application/json"}),"x-telemesh-token":this.token,...(init.headers||{})}});
  if(!r.ok){const body=await r.text();try{const j=JSON.parse(body);throw new Error(j.error||body||r.statusText)}catch(e){if(e instanceof Error)throw e;throw new Error(body||r.statusText)}}
  return r.json();
 }
 health(){return this.req<Health>("/health")} me(){return this.req<Me>("/api/v1/me")} dialogs(){return this.req<Dialog[]>("/api/v1/dialogs")}
 messages(peer:string,limit=50,offsetId=0){return this.req<{messages:Message[];has_more:boolean}>(`/api/v1/messages?peer=${encodeURIComponent(peer)}&limit=${limit}&offset_id=${offsetId}`)}
 sendMedia(peer:string,file:File,caption=""){const f=new FormData();f.append("peer",peer);f.append("caption",caption);f.append("file",file);return this.req<{message_id:number;peer_id:number}>("/api/v1/messages/media",{method:"POST",headers:{"x-telemesh-token":this.token},body:f})}
 send(peer:string,text:string,reply_to?:number){return this.req<{message_id:number;peer_id:number}>("/api/v1/messages/send",{method:"POST",body:JSON.stringify({peer,text,reply_to:reply_to??null})})}
 edit(peer:string,message_id:number,text:string){return this.req<{ok:boolean}>("/api/v1/messages/edit",{method:"POST",body:JSON.stringify({peer,message_id,text})})}
 delete(peer:string,message_ids:number[]){return this.req<{deleted:number}>("/api/v1/messages/delete",{method:"POST",body:JSON.stringify({peer,message_ids})})}
 forward(source:string,destination:string,message_ids:number[]){return this.req<Message[]>("/api/v1/messages/forward",{method:"POST",body:JSON.stringify({source,destination,message_ids})})}
 react(peer:string,message_id:number,reaction?:string){return this.req<{ok:boolean}>("/api/v1/messages/react",{method:"POST",body:JSON.stringify({peer,message_id,reaction:reaction??null})})}
 downloadMedia(peer:string,messageId:number){return fetch(this.base.replace(/\/$/,"")+"/api/v1/messages/media/download?peer="+encodeURIComponent(peer)+"&message_id="+messageId,{headers:{"x-telemesh-token":this.token}}).then(async r=>{if(!r.ok)throw new Error(await r.text());return r.blob()})}
 markRead(peer:string){return this.req<{ok:boolean}>("/api/v1/messages/read",{method:"POST",body:JSON.stringify({peer})})}
 search(q:string,peer?:string,limit=50){const p=peer?"&peer="+encodeURIComponent(peer):"";return this.req<{messages:Message[]}>(`/api/v1/messages/search?q=${encodeURIComponent(q)}${p}&limit=${limit}`)}
 loginStart(phone:string){return this.req<{status:string;requires_code:boolean}>("/api/v1/auth/start",{method:"POST",body:JSON.stringify({phone})})}
 loginComplete(code:string){return this.req<Me>("/api/v1/auth/complete",{method:"POST",body:JSON.stringify({code})})}
 loginPassword(password:string){return this.req<Me>("/api/v1/auth/password",{method:"POST",body:JSON.stringify({password})})}
 async ticket(){return this.req<{ticket:string}>("/api/v1/events/ticket",{method:"POST"})}
 events(onEvent:(e:Event)=>void|Promise<void>,onState:(s:boolean)=>void){
   let stopped=false,ws:WebSocket|null=null,timer:number|undefined,attempt=0;
   const connect=async()=>{
     if(stopped)return;
     try{
       const {ticket}=await this.ticket();
       if(stopped)return;
       const u=new URL(this.base.replace(/^http/,"ws")+"/api/v1/events");u.searchParams.set("ticket",ticket);
       ws=new WebSocket(u);
       ws.onopen=()=>{attempt=0;onState(true)};
       ws.onmessage=e=>{try{void onEvent(JSON.parse(e.data))}catch{}};
       ws.onerror=()=>onState(false);
       ws.onclose=()=>{
         ws=null;onState(false);
         if(stopped)return;
         const delay=Math.min(15000,500*Math.pow(2,attempt++));
         timer=window.setTimeout(()=>void connect(),delay);
       };
     }catch{
       onState(false);
       if(stopped)return;
       const delay=Math.min(15000,500*Math.pow(2,attempt++));
       timer=window.setTimeout(()=>void connect(),delay);
     }
   };
   void connect();
   return ()=>{stopped=true;if(timer!==undefined)window.clearTimeout(timer);timer=undefined;ws?.close();ws=null;onState(false)};
 }
}
export function messageText(m:Message){return m.text||"(media)"}
