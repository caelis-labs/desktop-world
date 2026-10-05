export interface Reply<T=unknown>{id:string;protocol:string;world:string;result?:T;error?:{code:string;message:string;retry_class?:string};}
export class DesktopError extends Error {constructor(fault:{code:string;message:string},reply?:Reply);code:string;reply?:Reply;receipt?:unknown;}
export class SessionTransport {
  hello:{protocol:string;environment:{epoch:string};features:string[];input_mode:string;input_policy:string};
  static start(helper:string,args?:string[]):Promise<SessionTransport>;
  request<T=unknown>(channel:'desktop'|'host',op:string,args?:unknown,id?:string):Promise<Reply<T>>;
  desktop(request:{id?:string;op:string;args?:unknown}):Promise<Reply>;
  owner<T=unknown>(op:string,args?:unknown,id?:string):Promise<T>;
  reconcile<T=unknown>(id:string):Promise<Reply<T>>;
  close():Promise<void>;
}
