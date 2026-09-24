import {useCallback,useEffect,useMemo,useRef,useState} from "react";
import {sendNotification} from "@tauri-apps/plugin-notification";
import {TeleMeshApi} from "./api";
import type {Dialog,Event,Health,Me,Message} from "./types";

const DEFAULT_SERVER="http://127.0.0.1:8787";
const initials=(s:string)=>s.trim().split(/\s+/).slice(0,2).map(x=>x[0]).join("").toUpperCase()||"?";

export default function App(){
 const [server,setServer]=useState(localStorage.getItem("tm_server")||DEFAULT_SERVER);
 const [token,setToken]=useState(localStorage.getItem("tm_token")||"");
 const [connected,setConnected]=useState(false),[health,setHealth]=useState<Health|null>(null),[me,setMe]=useState<Me|null>(null);
 const [dialogs,setDialogs]=useState<Dialog[]>([]),[selected,setSelected]=useState<Dialog|null>(null),[messages,setMessages]=useState<Message[]>([]);
 const [query,setQuery]=useState(""),[draft,setDraft]=useState(""),[error,setError]=useState(""),[loading,setLoading]=useState(false),[settings,setSettings]=useState(!token);
 const [historyMore,setHistoryMore]=useState(false),[loadingMore,setLoadingMore]=useState(false),[reply,setReply]=useState<Message|null>(null),[editing,setEditing]=useState<Message|null>(null),[search,setSearch]=useState(""),[searchResults,setSearchResults]=useState<Message[]>([]),[file,setFile]=useState<File|null>(null);
 const [phone,setPhone]=useState(""),[code,setCode]=useState(""),[password,setPassword]=useState(""),[loginStep,setLoginStep]=useState<"phone"|"code"|"password">("phone"),[loginBusy,setLoginBusy]=useState(false);
 const api=useMemo(()=>new TeleMeshApi(server,token),[server,token]); const stop=useRef<(()=>void)|null>(null); const scrollRef=useRef<HTMLDivElement|null>(null); const selectedRef=useRef<Dialog|null>(null); const syncingRef=useRef(false); const connectionRunRef=useRef(0);

 const chooseDialog=(d:Dialog|null)=>{selectedRef.current=d;setSelected(d)};
 const resync=useCallback(async(d:Dialog|null=selectedRef.current)=>{
   if(syncingRef.current)return;
   syncingRef.current=true;
   try{
     const fresh=await api.dialogs();
     setDialogs(fresh);
     const current=d?fresh.find(x=>x.id===d.id):fresh.find(x=>x.id===selectedRef.current?.id);
     if(current){
       selectedRef.current=current; setSelected(current);
       const r=await api.messages(current.username||String(current.id),50,0);
       setMessages(r.messages); setHistoryMore(r.has_more);
       await api.markRead(current.username||String(current.id));
       setDialogs(ds=>ds.map(x=>x.id===current.id?{...x,unread_count:0}:x));
     }
   }catch(e){setError(e instanceof Error?e.message:"State resync failed")}finally{syncingRef.current=false}
 },[api]);

 const connect=useCallback(async()=>{
   const run=++connectionRunRef.current;
   stop.current?.();
   stop.current=null;
   setError("");
   setLoading(true);
   try{
     localStorage.setItem("tm_server",server);
     localStorage.setItem("tm_token",token);
     const h=await api.health();
     if(run!==connectionRunRef.current)return;
     setHealth(h);
     if(!h.telegram_authorized){
       setConnected(false);
       setSettings(true);
       return;
     }
     const [m,d]=await Promise.all([api.me(),api.dialogs()]);
     if(run!==connectionRunRef.current)return;
     setMe(m);
     setDialogs(d);
     chooseDialog(d[0]||null);
     setSettings(false);
     const cleanup=api.events(async(e:Event)=>{
       if(run!==connectionRunRef.current)return;
       if(e.type==="NewMessage"&&e.data){
         const msg=e.data as Message;
         setDialogs(ds=>ds.map(d=>d.id===msg.peer_id?{...d,last_message:msg,unread_count:d.id===selectedRef.current?.id?0:d.unread_count+1}:d));
         setMessages(x=>selectedRef.current?.id===msg.peer_id&&!x.some(y=>y.id===msg.id)?[...x,msg]:x);
         if(selectedRef.current?.id!==msg.peer_id){try{sendNotification({title:"TeleMesh",body:msg.text||"New message"})}catch{}}
       }else if(e.type==="MessageEdited"&&e.data){
         const msg=e.data as Message;
         setMessages(x=>x.map(m=>m.id===msg.id?msg:m));
         setDialogs(ds=>ds.map(d=>d.id===msg.peer_id?{...d,last_message:d.last_message?.id===msg.id?msg:d.last_message}:d));
       }else if(e.type==="MessagesDeleted"&&e.data){
         const x=e.data as {peer_id:number;message_ids:number[]};
         setMessages(ms=>ms.filter(m=>m.peer_id!==x.peer_id||!x.message_ids.includes(m.id)));
       }else if(e.type==="ReactionUpdated"&&e.data){
         const x=e.data as {peer_id:number;message_id:number};
         if(selectedRef.current?.id===x.peer_id)await resync(selectedRef.current);
       }else if(e.type==="Status"&&e.data){
         const x=e.data as {authorized:boolean};
         if(x.authorized)await resync();
       }else if(e.type==="Reconnecting"){
         setConnected(false);
       }
     },setConnected);
     if(run!==connectionRunRef.current){
       cleanup();
       return;
     }
     stop.current=cleanup;
   }catch(e){
     if(run===connectionRunRef.current){
       setError(e instanceof Error?e.message:"Connection failed");
       setConnected(false);
       setSettings(true);
     }
   }finally{
     if(run===connectionRunRef.current)setLoading(false);
   }
 },[api,server,token,resync]);

 const authorize=async()=>{
   setError("");setLoginBusy(true);try{await api.loginStart(phone.trim());setLoginStep("code")}
   catch(e){setError(e instanceof Error?e.message:"Login failed")}finally{setLoginBusy(false)}
 };
 const completeLogin=async()=>{
   setError("");setLoginBusy(true);try{
     try{await api.loginComplete(code.trim());setLoginStep("phone");setCode("");await connect()}
     catch(e){
       const msg=e instanceof Error?e.message:"Login failed";
       if(msg.toLowerCase().includes("2fa")||msg.toLowerCase().includes("password"))setLoginStep("password");
       else throw e;
     }
   }catch(e){setError(e instanceof Error?e.message:"Login failed")}finally{setLoginBusy(false)}
 };
 const completePassword=async()=>{
   setError("");setLoginBusy(true);try{await api.loginPassword(password);setPassword("");setLoginStep("phone");await connect()}
   catch(e){setError(e instanceof Error?e.message:"2FA failed")}finally{setLoginBusy(false)}
 };

 const loadHistory=async(d:Dialog)=>{setLoadingMore(false);setLoading(true);try{
   const peer=d.username||String(d.id); const r=await api.messages(peer,50,0); setMessages(r.messages);setHistoryMore(r.has_more);await api.markRead(d.username||String(d.id));setDialogs(ds=>ds.map(x=>x.id===d.id?{...x,unread_count:0}:x));
   requestAnimationFrame(()=>{if(scrollRef.current)scrollRef.current.scrollTop=scrollRef.current.scrollHeight});
 }catch(e){setError(e instanceof Error?e.message:"History failed")}finally{setLoading(false)}};

 useEffect(()=>{if(selected)loadHistory(selected);else setMessages([])},[selected?.id]);
 useEffect(()=>{
   if(!token)return;
   void connect();
   return ()=>{
     ++connectionRunRef.current;
     stop.current?.();
     stop.current=null;
   };
 },[token,connect]);

 const loadOlder=async()=>{if(!selected||!historyMore||loadingMore||messages.length===0)return;setLoadingMore(true);try{
   const r=await api.messages(selected.username||String(selected.id),50,messages[0].id);
   setMessages(x=>[...r.messages,...x]);setHistoryMore(r.has_more);
 }catch(e){setError(e instanceof Error?e.message:"History failed")}finally{setLoadingMore(false)}};

 const send=async()=>{if(!selected||(!draft.trim()&&!file))return;const text=draft.trim();setDraft("");try{if(file){const r=await api.sendMedia(selected.username||String(selected.id),file,text);setFile(null);setMessages(x=>x.some(m=>m.id===r.message_id)?x:[...x,{id:r.message_id,peer_id:r.peer_id,text:text||file.name,outgoing:true}]);return}
   if(editing){await api.edit(selected.username||String(selected.id),editing.id,text);setMessages(x=>x.map(m=>m.id===editing.id?{...m,text,edited:true}:m));setEditing(null);return}
   const r=await api.send(selected.username||String(selected.id),text,reply?.id);
   setMessages(x=>x.some(m=>m.id===r.message_id)?x:[...x,{id:r.message_id,peer_id:r.peer_id,text,outgoing:true,reply_to:reply?.id??null}]);
   setReply(null);
   setDialogs(ds=>ds.map(d=>d.id===r.peer_id?{...d,last_message:{id:r.message_id,peer_id:r.peer_id,text,outgoing:true}}:d));
 }catch(e){setError(e instanceof Error?e.message:"Send failed");setDraft(text)}};

 const deleteMessage=async(m:Message)=>{if(!selected)return;try{await api.delete(selected.username||String(selected.id),[m.id]);setMessages(x=>x.filter(v=>v.id!==m.id))}catch(e){setError(e instanceof Error?e.message:"Delete failed")}};
 const react=async(m:Message)=>{if(!selected)return;try{await api.react(selected.username||String(selected.id),m.id,"❤️");setMessages(x=>x.map(v=>v.id===m.id?{...v,reaction_count:(v.reaction_count||0)+1}:v))}catch(e){setError(e instanceof Error?e.message:"Reaction failed")}};
 const runSearch=async()=>{if(!search.trim())return;try{const r=await api.search(search,selected?.username||String(selected?.id||""));setSearchResults(r.messages)}catch(e){setError(e instanceof Error?e.message:"Search failed")}};
 const filtered=dialogs.filter(d=>(d.name+" "+(d.username||"")).toLowerCase().includes(query.toLowerCase()));

 if(settings)return <div className="setup"><div className="setup-card"><div className="brand"><div className="logo">T</div><div><b>TeleMesh</b><span>Linux client</span></div></div><h1>Connect to your TeleMesh server</h1><p className="muted">Telegram credentials stay on the server. This client only needs the server endpoint and client token.</p>{error&&<div className="error">{error}</div>}<label>Server URL<input value={server} onChange={e=>setServer(e.target.value)} placeholder={DEFAULT_SERVER}/></label><label>Client token<input type="password" value={token} onChange={e=>setToken(e.target.value)} placeholder="TELEMESH_TOKEN"/></label><button className="primary" onClick={connect} disabled={loading}>{loading?"Connecting…":"Connect"}</button>{health&&!health.telegram_authorized&&<div className="login-box"><b>Telegram authorization</b><p className="muted">Enter the phone number for the Telegram account stored by this TeleMesh server.</p>{loginStep==="phone"&&<><input className="login-input" value={phone} onChange={e=>setPhone(e.target.value)} placeholder="+989123456789"/><button className="primary secondary" onClick={authorize} disabled={loginBusy||!phone.trim()}>{loginBusy?"Sending code…":"Send Telegram code"}</button></>}{loginStep==="code"&&<><input className="login-input" value={code} onChange={e=>setCode(e.target.value)} placeholder="Login code"/><button className="primary secondary" onClick={completeLogin} disabled={loginBusy||!code.trim()}>{loginBusy?"Verifying…":"Verify code"}</button></>}{loginStep==="password"&&<><input className="login-input" type="password" value={password} onChange={e=>setPassword(e.target.value)} placeholder="2FA password"/><button className="primary secondary" onClick={completePassword} disabled={loginBusy||!password}>{loginBusy?"Checking…":"Complete login"}</button></>}</div>}</div></div>;

 return <div className="app">
  <aside className="sidebar">
   <div className="side-head"><div className="brand"><div className="logo">T</div><div><b>TeleMesh</b><span>{connected?"Connected":"Offline"}</span></div></div><button className="icon" onClick={()=>setSettings(true)} title="Settings">⚙</button></div>
   <div className="me">{me&&<><div className="avatar">{initials(me.first_name||me.username||"U")}</div><div><b>{[me.first_name,me.last_name].filter(Boolean).join(" ")||me.username||"Telegram"}</b><small>@{me.username||"account"}</small></div><i className={connected?"online":""}/></>}</div>
   <div className="search"><span>⌕</span><input placeholder="Search chats" value={query} onChange={e=>setQuery(e.target.value)}/></div>
   <div className="section-title">Chats <span>{filtered.length}</span></div>
   <div className="dialogs">{filtered.map(d=><button className={"dialog "+(selected?.id===d.id?"active":"")} key={d.id} onClick={()=>chooseDialog(d)}><div className="avatar small">{initials(d.name)}</div><div className="dialog-body"><div><b>{d.name}</b><time>{d.last_message?.date?new Date(d.last_message.date).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit"}):""}</time></div><p>{d.last_message?.text||d.username||d.kind}{d.unread_count>0&&<strong>{d.unread_count}</strong>}</p></div></button>)}</div>
  </aside>
  <main className="chat">
   {selected?<><header className="chat-head"><div className="avatar">{initials(selected.name)}</div><div><h2>{selected.name}</h2><span>{selected.username?"@"+selected.username:selected.kind}</span></div><div className="head-actions"><input className="chat-search" value={search} onChange={e=>setSearch(e.target.value)} onKeyDown={e=>{if(e.key==="Enter")runSearch()}} placeholder="Search"/><button className="icon" onClick={runSearch}>⌕</button><button className="icon">⋮</button></div></header>
   <div className="messages" ref={scrollRef} onScroll={e=>{if(e.currentTarget.scrollTop<80)loadOlder()}}>{loadingMore&&<div className="muted center">Loading older messages…</div>}{messages.map(m=><div className={"row "+(m.outgoing?"out":"")} key={m.id}><div className="bubble">{m.reply_to&&<div className="reply-ref">↩ #{m.reply_to}</div>}<div>{m.text||"(media)"}</div>{m.edited&&<em> edited</em>}<small>{m.date?new Date(m.date).toLocaleTimeString([],{hour:"2-digit",minute:"2-digit"}):""}</small><div className="message-tools">{m.media&&<button onClick={async()=>{try{const b=await api.downloadMedia(selected.username||String(selected.id),m.id);const u=URL.createObjectURL(b);const a=document.createElement("a");a.href=u;a.download="telemesh-media";a.click();URL.revokeObjectURL(u)}catch(e){setError(e instanceof Error?e.message:"Download failed")}}}>Download</button>}{m.outgoing&&<><button onClick={()=>{setEditing(m);setDraft(m.text)}}>Edit</button><button onClick={()=>deleteMessage(m)}>Delete</button></>}<button onClick={()=>react(m)}>❤️</button><button onClick={()=>setReply(m)}>↩</button></div></div></div>)}{loading&&!messages.length&&<div className="muted center">Loading messages…</div>}</div>
   {searchResults.length>0&&<div className="search-results"><b>Search results</b>{searchResults.map(m=><button key={m.id} onClick={()=>setSearchResults([])}>{m.text||"(media)"}</button>)}</div>}{reply&&<div className="reply-bar">Replying to #{reply.id}<button onClick={()=>setReply(null)}>×</button></div>}{editing&&<div className="reply-bar">Editing #{editing.id}<button onClick={()=>setEditing(null)}>×</button></div>}<div className="composer"><label className="attach"><input type="file" onChange={e=>setFile(e.target.files?.[0]||null)}/>📎</label><textarea value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();send()}}} placeholder="Write a message…"/><button className="send" onClick={send} disabled={(!draft.trim()&&!file)||loading}>➤</button></div>
   </>:<div className="empty"><div className="empty-logo">T</div><h2>TeleMesh</h2><p>Select a chat to start messaging.</p></div>}
  </main>
  {error&&<button className="toast" onClick={()=>setError("")}>{error} ×</button>}
  <div className="status">{health?.telegram_authorized?"Telegram authorized":"Telegram not authorized"} · v{health?.version||"—"}</div>
 </div>
}
