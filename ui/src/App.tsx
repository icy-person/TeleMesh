import {useEffect,useMemo,useRef,useState} from "react";
import {TeleMeshApi} from "./api";
import type {Dialog,Event,Health,Me,Message} from "./types";

const DEFAULT_SERVER="http://127.0.0.1:8787";
const initials=(s:string)=>s.trim().split(/\\s+/).slice(0,2).map(x=>x[0]).join("").toUpperCase()||"?";

export default function App(){
 const [server,setServer]=useState(localStorage.getItem("tm_server")||DEFAULT_SERVER);
 const [token,setToken]=useState(localStorage.getItem("tm_token")||"");
 const [connected,setConnected]=useState(false),[health,setHealth]=useState<Health|null>(null),[me,setMe]=useState<Me|null>(null);
 const [dialogs,setDialogs]=useState<Dialog[]>([]),[selected,setSelected]=useState<Dialog|null>(null),[messages,setMessages]=useState<Message[]>([]);
 const [query,setQuery]=useState(""),[draft,setDraft]=useState(""),[error,setError]=useState(""),[settings,setSettings]=useState(!token);
 const api=useMemo(()=>new TeleMeshApi(server,token),[server,token]); const stop=useRef<(()=>void)|null>(null);

 const connect=async()=>{setError("");try{
   localStorage.setItem("tm_server",server);localStorage.setItem("tm_token",token);
   const [h,m,d]=await Promise.all([api.health(),api.me(),api.dialogs()]);
   setHealth(h);setMe(m);setDialogs(d);setSelected(d[0]||null);setSettings(false);
   stop.current=await api.events((e:Event)=>{if(e.type==="NewMessage"&&e.data){const msg=e.data as Message;setMessages(x=>x.some(y=>y.id===msg.id&&y.peer_id===msg.peer_id)?x:[...x,msg]);}},setConnected);
 }catch(e){setError(e instanceof Error?e.message:"Connection failed");setConnected(false)}};
 useEffect(()=>()=>stop.current?.(),[]);
 useEffect(()=>{if(!selected)return;setMessages(selected.last_message?[selected.last_message]:[])},[selected?.id]);

 const send=async()=>{if(!selected||!draft.trim())return;const text=draft.trim();setDraft("");try{
   const r=await api.send(selected.username||String(selected.id),text);
   setMessages(x=>[...x,{id:r.message_id,peer_id:r.peer_id,text,outgoing:true}]);
 }catch(e){setError(e instanceof Error?e.message:"Send failed");setDraft(text)}};
 const filtered=dialogs.filter(d=>(d.name+" "+(d.username||"")).toLowerCase().includes(query.toLowerCase()));

 if(settings)return <div className="setup"><div className="setup-card"><div className="brand"><div className="logo">T</div><div><b>TeleMesh</b><span>Linux client</span></div></div><h1>Connect to your TeleMesh server</h1><p className="muted">Telegram credentials stay on the server. This client only needs the server endpoint and client token.</p>{error&&<div className="error">{error}</div>}<label>Server URL<input value={server} onChange={e=>setServer(e.target.value)} placeholder={DEFAULT_SERVER}/></label><label>Client token<input type="password" value={token} onChange={e=>setToken(e.target.value)} placeholder="TELEMESH_TOKEN"/></label><button className="primary" onClick={connect}>Connect</button></div></div>;

 return <div className="app">
  <aside className="sidebar">
   <div className="side-head"><div className="brand"><div className="logo">T</div><div><b>TeleMesh</b><span>{connected?"Connected":"Offline"}</span></div></div><button className="icon" onClick={()=>setSettings(true)} title="Settings">⚙</button></div>
   <div className="me">{me&&<><div className="avatar">{initials(me.first_name||me.username||"U")}</div><div><b>{[me.first_name,me.last_name].filter(Boolean).join(" ")||me.username||"Telegram"}</b><small>@{me.username||"account"}</small></div>}<i className={connected?"online":""}/></div>}
   <div className="search"><span>⌕</span><input placeholder="Search chats" value={query} onChange={e=>setQuery(e.target.value)}/></div>
   <div className="section-title">Chats <span>{filtered.length}</span></div>
   <div className="dialogs">{filtered.map(d=><button className={"dialog "+(selected?.id===d.id?"active":"")} key={d.id} onClick={()=>setSelected(d)}><div className="avatar small">{initials(d.name)}</div><div className="dialog-body"><div><b>{d.name}</b><time>{d.last_message?.date?new Date(d.last_message.date).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit"}):""}</time></div><p>{d.last_message?.text||d.username||d.kind}</p></div></button>)}</div>
  </aside>
  <main className="chat">
   {selected?<><header className="chat-head"><div className="avatar">{initials(selected.name)}</div><div><h2>{selected.name}</h2><span>{selected.username?"@"+selected.username:selected.kind}</span></div><div className="head-actions"><button className="icon">⌕</button><button className="icon">⋮</button></div></header>
   <div className="messages">{messages.map(m=><div className={"row "+(m.outgoing?"out":"")} key={m.id}><div className="bubble">{m.text}<small>{m.date?new Date(m.date).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit"}):""}</small></div></div>)}</div>
   <div className="composer"><textarea value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();send()}}} placeholder="Write a message…"/><button className="send" onClick={send} disabled={!draft.trim()}>➤</button></div>
   </>:<div className="empty"><div className="empty-logo">T</div><h2>TeleMesh</h2><p>Select a chat to start messaging.</p></div>}
  </main>
  {error&&<button className="toast" onClick={()=>setError("")}>{error} ×</button>}
  <div className="status">{health?.telegram_authorized?"Telegram authorized":"Telegram not authorized"} · v{health?.version||"—"}</div>
 </div>
}
