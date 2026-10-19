import { reactive } from 'vue'
import { execute,errorText } from './bridge'
export type Page='overview'|'topology'|'network'|'routes'|'forward'|'socks'|'vpn'|'containers'|'services'
export interface Guest {ID:string;Name:string;State:string;OS:string;GuestTools?:{state:string};Network?:{ipAddresses:{ip:string,type:string}[]};Hardware?:Record<string,any>}
export interface Service {kind:string;location?:string;ip:string;port:number;target:string;allow:string;managed:boolean;running:boolean;status:string;stats?:Record<string,number>;events?:Record<string,any>[];vm?:string;vm_id?:string;telemetry?:string;last_error?:string}
export const state=reactive({page:'forward' as Page,vms:[] as Guest[],vm:'',host:null as any,mac:[] as Service[],guest:[] as Service[],pending:0,error:'',status:'就绪',serviceErrors:{mac:'',guest:''},output:'',selectedRule:'',readAt:0,loaded:false,backendOld:false})
export const guest=()=>state.vms.find(v=>v.ID===state.vm)
export const guestReady=()=>guest()?.State==='running'
export const guestArgs=(...args:string[])=>['--vm',state.vm,...args]
export function serviceId(r:Service){return `${r.location==='host'?'host':'guest'}:${r.kind}:${r.ip}:${r.port}`}
export async function run(purpose:string,args:string[]=[],quiet=false){state.pending++;try{const r=await execute(purpose,args);if(!quiet){state.error='';state.output=r.text;state.status='操作完成'}return r.data??r.text}catch(e){if(!quiet){state.error=errorText(e);state.status='操作失败'}throw e}finally{state.pending--}}
export async function refreshVms(){try{const value=await run('vms',[],true);state.vms=Array.isArray(value)?value:[];state.loaded=true}catch(e){state.error=errorText(e)}}
export async function refreshHost(){try{state.host=await run('host-read');state.vms=state.host.vms??state.vms}catch{}}
export async function refreshServices(){const selected=state.vm;await Promise.all([
 run('cli',['host-forward','list'],true).then(v=>{state.mac=v.services??[];state.backendOld=!v.backend_build||v.backend_build!==v.client_build;state.serviceErrors.mac='';state.readAt=Date.now()}).catch(e=>state.serviceErrors.mac=errorText(e)),
 guestReady()?run('cli',guestArgs('services','list'),true).then(v=>{if(state.vm===selected){state.guest=v.services??[];state.serviceErrors.guest=''}}).catch(e=>{if(state.vm===selected)state.serviceErrors.guest=errorText(e)}):Promise.resolve()
 ])}
export async function changeService(row:Service,action:'start'|'stop'|'delete'|'check'){
 const host=row.location==='host';const ep=`${row.ip}:${row.port}`;let args:string[]
 if(host)args=['host-forward',action,'--listen',ep]
 else{const op=action==='start'?(row.kind==='forward'?'add':'start'):action==='stop'?(row.kind==='forward'?'remove':'stop'):'delete';args=guestArgs(row.kind,op,'--listen',ep);if(action==='start'){args.push('--allow',row.allow);if(row.kind==='forward')args.push('--target',row.target)}}
 try{const result=await run('cli',args);if(action==='check'&&result.target_tcp_ok===false)state.error=result.error||'Mac 到实际目标不可达';await refreshServices();return result}catch{}}
export async function addRules(location:string,rules:{ip:string,port:string,target:string,allow:string}[],kind='forward',gost=''){
 if(!rules.length)throw Error('请填写至少一条规则')
 const rows=rules.map((r,index)=>{if(!r.ip||!/^\d+$/.test(r.port)||Number(r.port)<1||Number(r.port)>65535)throw Error(`第 ${index+1} 行：入口 IP 或端口无效`);if(kind==='forward'&&!/^\d+\.\d+\.\d+\.\d+:\d+$/.test(r.target))throw Error(`第 ${index+1} 行：实际目标必须为 IPv4:端口`);return r})
 if(location==='host')await run('cli',['host-forward','batch','--request',JSON.stringify(rows.map(r=>({listen:`${r.ip}:${r.port}`,target:r.target,vm:guest()?.Name??'',vm_id:state.vm})))])
 else{if(!guestReady())throw Error('请先选择运行中的虚拟机');const request=rows.map(r=>[kind,kind==='forward'?'add':'start','--listen',`${r.ip}:${r.port}`,'--allow',r.allow,...kind==='forward'?['--target',r.target]:[],...gost?['--gost',gost]:[]]);await run('cli',guestArgs('services','batch','--request',JSON.stringify(request)))}
 await refreshServices()
}
