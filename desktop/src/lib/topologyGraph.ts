export interface TopologyNode {id:string;title:string;sub:string;kind:string;x:number;y:number;data?:any;service?:any;vm?:string;machineId?:string}
export interface TopologyEdge {from:string;to:string;label:string;kind:'network'|'ownership'|'forward'|'socks'|'allowed'|'blocked'|'unknown'}
interface Client {id:string;name:string;os:string;state:string;ips:string[]}
interface GraphInput {view:string;query:string;host:any;vms:any[];services:any[];clients:Client[];positions:Record<string,{x:number;y:number}>}
export const NODE_WIDTH=248,NODE_HEIGHT=112
export const COLUMNS=[24,384,744,1104]
export function allowed(ips:string[],allow:string){
 let valid=false
 for(const p of allow.split(',')){
  const [ip,b]=p.trim().split('/'),bits=Number(b),a=ip?.split('.').map(Number)
  if(!a||a.length!==4||a.some(n=>!Number.isInteger(n)||n<0||n>255)||b===undefined||!Number.isInteger(bits)||bits<0||bits>32)continue
  valid=true
  const n=a.reduce((s,v)=>(s*256+v)>>>0,0),mask=bits===0?0:(0xffffffff<<(32-bits))>>>0
  if(ips.some(s=>{const oct=s.split('.').map(Number);return oct.length===4&&oct.every(v=>Number.isInteger(v)&&v>=0&&v<=255)&&((oct.reduce((a,v)=>(a*256+v)>>>0,0)&mask)>>>0)===((n&mask)>>>0)}))return '规则允许'
 }
 return valid&&ips.length?'已知 IP 未允许':'待确认'
}
export function buildTopology(input:GraphInput){
 const {view,host,vms,services,clients,positions}=input,q=input.query.trim().toLowerCase()
 const nodes:TopologyNode[]=[],edges:TopologyEdge[]=[]
 const machine=(id:string)=>id==='host'?{OS:'macos',State:'running'}:vms.find(v=>v.ID===id)
 const machineName=(id:string)=>id==='host'?'本地 Mac':vms.find(v=>v.ID===id)?.Name||id
 const serviceRows=services.filter(r=>`${r.ownerName} ${r.ip}:${r.port} ${r.target} ${r.kind==='socks'?'SOCKS5 代理':'TCP 端口转发'}`.toLowerCase().includes(q))
 if(view!=='network'){
  let rowY=80
  for(const r of serviceRows){
   const sid=`service:${r.ownerId}:${r.kind}:${r.ip}:${r.port}`
   const visibleClients=view==='access'?clients:clients.filter(c=>allowed(c.ips,r.allow)==='规则允许')
   const sources=visibleClients.length?visibleClients:[{id:'unknown',name:'允许来源',os:'',state:'',ips:[]}]
   const targetValues:string[]=r.kind==='socks'?(r.active_targets||[]).filter((t:any)=>t.connections>0).map((t:any)=>t.target):[r.target]
   const targets=targetValues.length?targetValues:['客户端指定目标']
   const count=Math.max(sources.length,targets.length),center=rowY+(count-1)*80
   sources.forEach((c,i)=>{
    const id=`source:${sid}:${c.id}`,policy=c.id==='unknown'?'待确认':allowed(c.ips,r.allow)
    const sourceIPs=view==='flow'?c.ips.filter(ip=>allowed([ip],r.allow)==='规则允许'):c.ips
    nodes.push({id,title:c.name,sub:sourceIPs.join(', ')||r.allow||'暂无来源规则',kind:'访问来源',x:COLUMNS[0]!,y:rowY+i*160,data:c,machineId:c.id==='unknown'?undefined:c.id})
    edges.push({from:id,to:sid,label:policy==='规则允许'?'允许访问':policy,kind:policy==='规则允许'&&!r.stale?'allowed':policy==='已知 IP 未允许'?'blocked':'unknown'})
   })
   nodes.push({id:sid,title:`${r.ip}:${r.port}`,sub:`运行于：${r.ownerName}${r.stale?' · 旧快照':''}`,kind:r.kind==='socks'?'SOCKS5 代理入口':'TCP 端口转发入口',x:COLUMNS[1]!,y:center,service:r,vm:r.ownerId,machineId:r.ownerId})
   targets.forEach((target,i)=>{
    const ip=target.split(':')[0],owner=ip?.startsWith('127.')?r.ownerId:clients.find(c=>c.ips.includes(ip))?.id||vms.find(v=>v.Network?.ipAddresses?.some((a:any)=>a.ip===ip))?.ID
    const id=`target:${sid}:${i}`
    nodes.push({id,title:target,sub:owner?`目标机器：${machineName(owner)}`:r.kind==='socks'&&!targetValues.length?'等待客户端请求 · TCP CONNECT':'由入口所在机器连接',kind:'实际目标',x:COLUMNS[2]!,y:rowY+i*160,service:r,vm:owner,machineId:owner,data:owner?machine(owner):undefined})
    edges.push({from:sid,to:id,label:r.kind==='socks'?targetValues.length?'当前 TCP 目标':'客户端指定':'TCP 转发',kind:r.kind==='socks'?'socks':'forward'})
   })
   rowY+=count*160+64
  }
 }else{
  const interfaces=(host?.interfaces||[]).filter((i:any)=>i.ips?.length)
  const filtered=vms.filter(v=>`${v.Name} ${v.OS} ${v.Network?.ipAddresses?.map((i:any)=>i.ip).join(' ')}`.toLowerCase().includes(q))
  nodes.push({id:'host',title:'本地 Mac',sub:'宿主机 · Parallels Desktop',kind:'宿主机',x:COLUMNS[0]!,y:152,machineId:'host'})
  interfaces.forEach((i:any,n:number)=>{
   nodes.push({id:`if:${i.device}`,title:i.name||i.device,sub:`${i.device} · ${i.ips.join(', ')}`,kind:'Mac 网卡',x:COLUMNS[1]!,y:80+n*160,data:i,machineId:'host'})
   edges.push({from:'host',to:`if:${i.device}`,label:'接口归属',kind:'network'})
  })
  let vmY=80
  filtered.forEach(v=>{
   nodes.push({id:v.ID,title:v.Name,sub:(v.Network?.ipAddresses||[]).filter((a:any)=>a.type==='ipv4').map((a:any)=>a.ip).join(', ')||'暂无已知 IPv4',kind:'虚拟机',x:COLUMNS[2]!,y:vmY,vm:v.ID,machineId:v.ID,data:v})
   vmY+=160+services.filter(r=>r.ownerId===v.ID).length*144
   const adapters=Object.values(v.Hardware||{}).filter((a:any)=>['shared','host-only','bridged'].includes(a?.type)) as any[]
   const links=adapters.length?adapters:[{type:'unknown'}]
   const seen=new Set<string>()
   for(const adapter of links){
    const iface=adapter.type==='shared'?'bridge100':adapter.type==='host-only'?'bridge101':adapter.iface
    const known=interfaces.some((a:any)=>a.device===iface)
    const from=known?`if:${iface}`:`net:${adapter.type}:${iface||'unmapped'}`
    if(seen.has(from))continue;seen.add(from)
    const label=adapter.type==='shared'?'共享 NAT':adapter.type==='host-only'?'仅主机网络':adapter.type==='bridged'?'桥接网络':'配置未确认'
    if(!nodes.some(n=>n.id===from)){
     nodes.push({id:from,title:label,sub:'尚未确认对应 Mac 网卡',kind:'虚拟网络',x:COLUMNS[1]!,y:80+nodes.filter(n=>n.id.startsWith('if:')||n.id.startsWith('net:')).length*160,machineId:'host'})
     edges.push({from:'host',to:from,label:'配置归属',kind:'network'})
    }
    edges.push({from,to:v.ID,label,kind:'network'})
   }
  })
  // Services stay next to their actual owner. Target arrows are separate from
  // the neutral network hierarchy, so a Mac listener never appears behind a VM.
  let externalY=80
  for(const owner of nodes.filter(n=>n.id==='host'||filtered.some(v=>v.ID===n.id))){
   services.filter(r=>r.ownerId===owner.id).forEach((r,i)=>{
    const id=`service:${r.ownerId}:${r.kind}:${r.ip}:${r.port}`
    const sy=owner.y+160+i*144
    nodes.push({id,title:`${r.ip}:${r.port}`,sub:`运行于：${r.ownerName}${r.stale?' · 旧快照':''}`,kind:r.kind==='socks'?'SOCKS5 代理入口':'TCP 端口转发入口',x:owner.x,y:sy,service:r,vm:r.ownerId,machineId:r.ownerId})
    edges.push({from:owner.id,to:id,label:'运行于此机器',kind:'ownership'})
    const targets:string[]=r.kind==='socks'?(r.active_targets||[]).filter((t:any)=>t.connections>0).map((t:any)=>t.target):[r.target]
    if(!targets.length){targets.push('客户端指定目标')}
    targets.forEach((target,j)=>{
     const ip=target.split(':')[0],targetId=ip?.startsWith('127.')?r.ownerId:clients.find(c=>c.ips.includes(ip))?.id||vms.find(v=>v.Network?.ipAddresses?.some((a:any)=>a.ip===ip))?.ID
     let to=targetId&&nodes.some(n=>n.id===targetId)?targetId:''
     if(!to){
      to=`external:${id}:${j}`;externalY=Math.max(externalY,sy)
      nodes.push({id:to,title:target,sub:targetId?`目标机器：${machineName(targetId)}`:r.kind==='socks'&&!r.active_targets?.length?'等待客户端请求 · TCP CONNECT':'实际服务目标',kind:'实际目标',x:COLUMNS[2]!+360,y:externalY,service:r,vm:targetId,machineId:targetId,data:targetId?machine(targetId):undefined})
      externalY+=144
     }
     edges.push({from:id,to,label:r.kind==='socks'?(target==='客户端指定目标'?'客户端指定':'SOCKS5 目标'):`TCP → ${target.split(':').pop()}`,kind:r.kind==='socks'?'socks':'forward'})
    })
   })
  }
 }
 for(const n of nodes){const saved=positions[`${view}:${n.id}`];if(saved){n.x=saved.x;n.y=saved.y}}
 return {nodes,edges,width:Math.max(1024,...nodes.map(n=>n.x+NODE_WIDTH+32)),height:Math.max(320,...nodes.map(n=>n.y+NODE_HEIGHT+36))}
}
