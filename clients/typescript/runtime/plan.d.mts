import type {Target,Step,ActOptions,Locator} from '../src/types.js';
export class PlanBuilder {
  steps:Step[];
  add(step:Step):this;
  focus(ref:string):this;
  bindFocus(name:string,within:string):{bound:string};
  bind(name:string,locator:Locator):{bound:string};
  press(ref:string|Target,key:string,modifiers?:string[]):this;
  type(ref:string|Target,text:string):this;
  set(ref:string,text:string):this;
  invoke(ref:string):this;
  click(ref:string|Target,options?:{button?:'left'|'right'|'middle';count?:1|2}):this;
  build(options?:ActOptions):ActOptions&{steps:Step[]};
}
