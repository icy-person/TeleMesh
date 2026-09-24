export type Dialog={id:number;name:string;username?:string|null;kind:string;last_message?:Message|null;unread_count:number};
export type Message={id:number;peer_id:number;text:string;outgoing:boolean;date?:string|null;reply_to?:number|null;edited?:boolean;reaction_count?:number|null;media?:{kind:string;filename?:string|null;mime?:string|null;size?:number|null}|null};
export type Event=(
 {type:"NewMessage"|"MessageEdited";data:Message}|
 {type:"MessagesDeleted";data:{peer_id:number;message_ids:number[]}}|
 {type:"ReactionUpdated";data:{peer_id:number;message_id:number}}|
 {type:"Status";data:{authorized:boolean}}|
 {type:"Connected"|"Reconnecting";data?:undefined}
);
export type Health={status:string;telegram_authorized:boolean;version:string};
export type Me={id:number;username?:string|null;first_name?:string|null;last_name?:string|null};
