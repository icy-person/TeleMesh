export type Dialog={id:number;name:string;username?:string|null;kind:string;last_message?:Message|null};
export type Message={id:number;peer_id:number;text:string;outgoing:boolean;date?:string|null};
export type Event={type:string;data?:Message|{authorized:boolean}};
export type Health={status:string;telegram_authorized:boolean;version:string};
export type Me={id:number;username?:string|null;first_name?:string|null;last_name?:string|null};
