import { reactive } from 'vue'
const draft = (location:string) => ({location,forms:{host:{ip:'127.0.0.1',port:'',target:'',batch:false,text:''},guest:{ip:'',port:'',target:'',batch:false,text:''}},allow:'',accessMode:'single',sources:[] as string[],gost:''})
export const forwardDrafts=reactive({forward:draft('host'),socks:draft('guest')})
