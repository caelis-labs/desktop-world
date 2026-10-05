const target=value=>typeof value==='string'?{ref:value}:structuredClone(value);
export class PlanBuilder {
  constructor(){this.steps=[];}
  add(step){if(this.steps.length>=16)throw new Error('A native plan has at most 16 steps.');this.steps.push({...structuredClone(step),id:step.id??`s${this.steps.length+1}`});return this;}
  focus(ref){return this.add({op:'focus',target:target(ref)});}
  bindFocus(name,within){this.add({op:'bind_focus',bind_focus:{name,within}});return {bound:name};}
  bind(name,locator){this.add({op:'bind',bind:{name,locator,require_unique:true}});return {bound:name};}
  press(ref,key,modifiers=[]){return this.add({op:'keyboard.press',target:target(ref),press:{key,modifiers}});}
  type(ref,text){return this.add({op:'keyboard.type_text',target:target(ref),type_text:{text}});}
  set(ref,text){return this.add({op:'set_value',target:target(ref),set_value:{text}});}
  invoke(ref){return this.add({op:'invoke',target:target(ref)});}
  click(ref,options={}){return this.add({op:'pointer.click',target:target(ref),click:{button:'left',count:1,...options}});}
  build(options={}){return {...options,steps:structuredClone(this.steps)};}
}
