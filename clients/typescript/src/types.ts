export type Ref = string;
export type Fact<T> = {known:T}|{status:'known';value:T;source?:string;sampled_at?:string}|{status:'unknown'|'unsupported'|'redacted';reason?:string};
export interface Fault {code:string;message:string;retry_class?:string;[key:string]:unknown;}
export interface UIObject {ref:Ref;kind:'application'|'window'|'ui';role?:string;name?:Fact<string>;app?:Ref;window?:Ref;parent?:Ref;value_preview?:Fact<string>;uri?:Fact<string>;states?:Record<string,Fact<boolean>>;capabilities?:unknown[];version?:string;geometry_version?:string;lifecycle?:string;[key:string]:unknown;}
export interface Coverage {complete:boolean;dirty?:boolean;truncated?:boolean;continuation?:string;unavailable_sources?:string[];[key:string]:unknown;}
export interface Observation {objects:UIObject[];coverage:Coverage;seat?:{focused_object?:Fact<Ref>;foreground_application?:Fact<Ref>;foreground_window?:Fact<Ref>;[key:string]:unknown};cursor?:string;[key:string]:unknown;}
export type Target={ref:Ref;bound?:never;anchor?:never;point?:never}|{bound:string;ref?:never;anchor?:never;point?:never}|{anchor:{target:Ref;u:number;v:number};ref?:never;bound?:never;point?:never};
export interface Locator {within:Ref;kind?:string;role?:string;name_equals?:string;name_contains?:string;required_states?:Record<string,boolean>;required_capability?:string;max_depth?:number;}
export interface Predicate {target:Target;property:string;equals_string?:string;equals_bool?:boolean;equals_version?:string;}
interface BaseStep {id?:string;completion?:'dispatch'|'verify';before?:Predicate[];after?:Predicate[];timeout_ms?:number;}
export type Step=BaseStep&(
 {op:'focus'|'invoke'|'scroll_into_view'|'pointer.move';target:Target}|
 {op:'bind';bind:{name:string;locator:Locator;require_unique:true}}|
 {op:'bind_focus';bind_focus:{name:string;within:Ref}}|
 {op:'wait';after:Predicate[]}|
 {op:'set_value';target:Target;set_value:{text:string}}|
 {op:'set_checked';target:Target;set_checked:{checked:boolean}}|
 {op:'set_expanded';target:Target;set_expanded:{expanded:boolean}}|
 {op:'set_selected';target:Target;set_selected:{selected:boolean}}|
 {op:'keyboard.press';target:Target;press:{key:string;modifiers:string[]}}|
 {op:'keyboard.type_text';target:Target;type_text:{text:string}}|
 {op:'pointer.click';target:Target;click:{button:'left'|'right'|'middle';count:1|2}}|
 {op:'pointer.scroll';target:Target;scroll:{dx:number;dy:number;unit:'wheel_step'}}|
 {op:'pointer.drag';target:Target;drag:{to:Target;duration_ms?:number}});
export interface ActOptions {timeout_ms?:number;}
export interface StepResult {id:string;state:string;delivery:string;verification:string;channel?:string;fault?:Fault;target?:Ref;[key:string]:unknown;}
export interface Receipt {run_id:string;outcome:'completed'|'partial'|'unknown'|'pending'|'failed'|'cancelled'|string;steps:StepResult[];seat_health?:string;input?:{foreground_ms?:number;restoration?:string;[key:string]:unknown};fault?:Fault;[key:string]:unknown;}
export interface ReadResult {text:Fact<string>;next?:string;truncated?:boolean;[key:string]:unknown;}
export interface Grant {id:string;application?:Ref;name?:string;window_title?:string;state:'pending'|'active'|'ambiguous'|'unresolved'|'expired'|'revoked';reason?:string;}
export interface GrantStatus {turn:string;grants:Grant[];}
export interface SessionOptions {helper:string;inputMode?:'shared'|'cooperative';inputPolicy?:'shared_input'|'no_shared_input';writeApps?:string[];writeAppWindows?:string[];assetsDir?:string;audit?:string;auditMode?:'rotate'|'append'|'create';ownerFile?:string;}
export interface ObserveArgs {scope?:{desktop:true}|{refs:Ref[]};projection?:'summary'|'outline'|'detail'|'capture_windows';fields?:string[];match?:Locator;budget?:Record<string,number>;continuation?:string;freshness?:{mode:string;max_age_ms?:number};}
