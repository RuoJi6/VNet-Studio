<script setup lang="ts">
import {computed,ref,reactive,onMounted,onUnmounted,nextTick,watch} from 'vue'
import {NetworkIcon,ArrowLeftRightIcon,RefreshCwIcon,PlusIcon,MinusIcon,SearchIcon,WifiIcon,EthernetPortIcon,Repeat2Icon,GlobeIcon,RotateCcwIcon,ShieldCheckIcon} from '@lucide/vue'
import {Button} from '@/components/ui/button'
import {Input} from '@/components/ui/input'
import {ToggleGroup,ToggleGroupItem} from '@/components/ui/toggle-group'
import {Alert,AlertDescription} from '@/components/ui/alert'
import MachineIcon from './MachineIcon.vue'
import {machineAppearance,machineStatus} from '@/lib/machine'
import AppSelect from './AppSelect.vue'
import {state,run,refreshHost,refreshServices,serviceId,type Service} from '@/lib/store'
import {forwardDrafts} from '@/lib/forwardState'
import {errorText} from '@/lib/bridge'
import {buildTopology,COLUMNS,type TopologyNode as Node,type TopologyEdge} from '@/lib/topologyGraph'
import {routeTopologyEdge} from '@/lib/topologyRouting'
import {bindTopologyZoom,MIN_ZOOM,MAX_ZOOM} from '@/lib/topologyZoom'
const canvas=ref<HTMLElement|null>(null);const pan=reactive({x:0,y:0});const positions=ref<Record<string,{x:number;y:number}>>({});const dragging=ref('');const panning=ref(false);
const selectedEdge=ref('');const edgeKey=(e:any)=>`${e.from}→${e.to}`;
const view=ref('network');const query=ref('');const zoom=ref(1);const selected=ref('');const snapshot=ref<Record<string,{rows:Service[];at:number;error:string}>>({});const loading=ref(false);const auto=ref(true)
const clients=computed(()=>[{id:'host',name:'本地 Mac',os:'macos',state:'running',ips:[...new Set<string>(['127.0.0.1',...(state.host?.interfaces||[]).flatMap((i:any)=>i.ips||[])])]},...state.vms.filter(v=>v.State==='running').map(v=>({id:v.ID,name:v.Name,os:v.OS,state:v.State,ips:(v.Network?.ipAddresses||[]).filter(i=>i.type==='ipv4').map(i=>i.ip)}))])
const services=computed(()=>[...state.mac.map(r=>({...r,ownerId:'host',ownerName:'本地 Mac',stale:!!state.serviceErrors.mac})),...Object.entries(snapshot.value).flatMap(([id,s])=>s.rows.filter(r=>r.running).map(r=>({...r,ownerId:id,ownerName:state.vms.find(v=>v.ID===id)?.Name||id,stale:Date.now()-s.at>30000||!!s.error})))].filter(r=>r.running))
const graph=computed(()=>buildTopology({view:view.value,query:query.value,host:state.host,vms:state.vms,services:services.value,clients:clients.value,positions:positions.value}))
const contextMachineId=ref(state.vm||'host')
const contextMachine=computed(()=>state.vms.find(v=>v.ID===contextMachineId.value))
const contextOptions=computed(()=>[{value:'host',label:'本地 Mac',icon:machineAppearance('macos').src},...state.vms.map(v=>({value:v.ID,label:`${v.Name} · ${machineStatus(v.State)}`,icon:machineAppearance(v.OS,v.Name).src}))])
watch(()=>state.vm,id=>contextMachineId.value=id||'host')
function setContext(id:string){contextMachineId.value=id;clearFocus()}
function newService(kind:'forward'|'socks'){
 if(kind==='socks'&&contextMachineId.value==='host')return
 forwardDrafts[kind].location=contextMachineId.value==='host'?'host':'guest'
 state.vm=contextMachineId.value==='host'?'':contextMachineId.value;state.selectedRule='';state.error='';state.page=kind
}
function openVPN(){if(contextMachineId.value==='host')return;state.vm=contextMachineId.value;state.error='';state.page='vpn'}
const laneLabels=computed(()=>view.value==='network'?['Mac / 本地服务','网卡 / 虚拟网络','虚拟机 / 来宾服务',...(graph.value.nodes.some(n=>n.x>=1104)?['外部 / 未定位目标']:[])]:['访问来源','转发 / 代理入口','实际目标'])
const routes=computed(()=>new Map(graph.value.edges.map(e=>[edgeKey(e),routeTopologyEdge(edgeEndpoint(e,'from'),edgeEndpoint(e,'to'),graph.value.nodes,e.label)])))
const edgeColors=['#64748b','#2563eb','#7c3aed','#15803d','#dc2626','#ea580c']
function edgeColor(e:TopologyEdge){return edgeSelected(e)?'#ea580c':({network:'#64748b',ownership:'#94a3b8',forward:'#2563eb',socks:'#7c3aed',allowed:'#15803d',blocked:'#dc2626',unknown:'#64748b'}[e.kind])}
function edgeDashed(e:TopologyEdge){return ['ownership','allowed','blocked','unknown'].includes(e.kind)}
watch(()=>graph.value.edges.map(edgeKey).join('|'),()=>{if(!graph.value.edges.some(e=>edgeKey(e)===selectedEdge.value))selectedEdge.value=''})
const focusedEdges=computed(()=>{
 const result=new Set<string>()
 if(selectedEdge.value){result.add(selectedEdge.value);return result}
 if(!selected.value)return result
 const node=graph.value.nodes.find(n=>n.id===selected.value)
 if(view.value==='network'&&node?.service){
  const serviceId=graph.value.nodes.find(n=>n.service===node.service&&n.kind!=='实际目标')?.id||node.id
  for(const e of graph.value.edges)if(e.from===serviceId||e.to===serviceId)result.add(edgeKey(e))
  return result
 }
 // Trace the neutral hierarchy independently from service connections. A
 // proxy returning to its owner must not turn selection into a whole-graph flood.
 const structure=view.value==='network'?graph.value.edges.filter(e=>e.kind==='network'):graph.value.edges
 for(const direction of ['from','to'] as const){
  const pending=[selected.value];const seen=new Set<string>()
  while(pending.length){const id=pending.pop()!;if(seen.has(id))continue;seen.add(id)
   for(const e of structure){if(e[direction]===id){result.add(edgeKey(e));pending.push(direction==='from'?e.to:e.from)}}
  }
 }
 if(view.value==='network'){
  const related=new Set(graph.value.edges.filter(e=>(e.kind==='ownership'&&e.from===selected.value)||(['forward','socks'].includes(e.kind)&&e.to===selected.value)).map(e=>e.kind==='ownership'?e.to:e.from))
  for(const e of graph.value.edges)if(related.has(e.from)||related.has(e.to))result.add(edgeKey(e))
 }

 return result
})
function focusEdge(e:any){if(performance.now()<suppressClickUntil)return;selectedEdge.value=edgeKey(e);selected.value=''}
function chooseNode(id:string){selected.value=id;selectedEdge.value='';const node=graph.value.nodes.find(n=>n.id===id);if(node?.machineId)contextMachineId.value=node.machineId}
function clearFocus(){selected.value='';selectedEdge.value=''}
let gesture:{pointer:number;kind:'node'|'pan';node?:string;startX:number;startY:number;originX:number;originY:number;moved:boolean;capture:HTMLElement}|null=null
let suppressClickUntil=0
let zoomControl:ReturnType<typeof bindTopologyZoom>|undefined
function pinch(){
 suppressClickUntil=performance.now()+350
 if(gesture?.capture.hasPointerCapture(gesture.pointer))gesture.capture.releasePointerCapture(gesture.pointer)
 gesture=null;dragging.value='';panning.value=false
}
function startNodeDrag(event:PointerEvent,n:Node){
 if(event.button!==0)return
 event.preventDefault();event.stopPropagation();chooseNode(n.id)
 const capture=event.currentTarget as HTMLElement;capture.focus({preventScroll:true});capture.setPointerCapture(event.pointerId)
 gesture={pointer:event.pointerId,kind:'node',node:n.id,startX:event.clientX,startY:event.clientY,originX:n.x,originY:n.y,moved:false,capture}
}
function startPan(event:PointerEvent){
 if(event.button!==0||(event.target as Element).closest('.topology-node,.topology-edge'))return
 event.preventDefault();const capture=event.currentTarget as HTMLElement;capture.focus({preventScroll:true});capture.setPointerCapture(event.pointerId)
 gesture={pointer:event.pointerId,kind:'pan',startX:event.clientX,startY:event.clientY,originX:pan.x,originY:pan.y,moved:false,capture}
}
function movePointer(event:PointerEvent){
 const g=gesture;if(!g||g.pointer!==event.pointerId)return
 const dx=event.clientX-g.startX,dy=event.clientY-g.startY
 if(!g.moved&&Math.hypot(dx,dy)<4)return
 g.moved=true;event.preventDefault()
 if(g.kind==='node'){
  dragging.value=g.node!;positions.value[`${view.value}:${g.node}`]={x:Math.max(16,g.originX+dx/zoom.value),y:Math.max(16,g.originY+dy/zoom.value)}
 }else{panning.value=true;pan.x=g.originX+dx;pan.y=g.originY+dy}
}
function endPointer(event:PointerEvent){
 if(!gesture||gesture.pointer!==event.pointerId)return
 if(gesture.moved)suppressClickUntil=performance.now()+350
 else if(gesture.kind==='pan')clearFocus()
 if(gesture.capture.hasPointerCapture(event.pointerId))gesture.capture.releasePointerCapture(event.pointerId)
 gesture=null;dragging.value='';panning.value=false
}
function clickNode(id:string,event:MouseEvent){if(performance.now()<suppressClickUntil)return;chooseNode(id)}
function backgroundClick(event:MouseEvent){
 if((event.target as Element).closest('.topology-node,.topology-edge')||performance.now()<suppressClickUntil)return
 clearFocus()
}
function resetLayout(){
 positions.value=Object.fromEntries(Object.entries(positions.value).filter(([key])=>!key.startsWith(view.value+':')))
 pan.x=0;pan.y=0;clearFocus();if(canvas.value){canvas.value.scrollLeft=0;canvas.value.scrollTop=0}
}
watch(view,()=>{pan.x=0;pan.y=0;clearFocus()})
function adjust(){const r=active.value?.service;if(!r)return;if(r.ownerId!=='host')state.vm=r.ownerId;const draft=r.kind==='socks'?forwardDrafts.socks:forwardDrafts.forward;draft.location=r.ownerId==='host'?'host':'guest';Object.assign(draft.forms[draft.location as 'host'|'guest'],{ip:r.ip,port:String(r.port),target:r.target,batch:false});draft.allow=r.allow;draft.accessMode='custom';state.selectedRule='';state.page=r.kind==='socks'?'socks':'forward'}
const edgeLabel=computed(()=>{const e=graph.value.edges.find(e=>edgeKey(e)===selectedEdge.value);return e?`${graph.value.nodes.find(n=>n.id===e.from)?.title} → ${graph.value.nodes.find(n=>n.id===e.to)?.title} · ${e.label}`:''})
const active=computed(()=>graph.value.nodes.find(n=>n.id===selected.value))
function nodeIcon(n:Node){
 if(n.kind==='实际目标')return n.machineId?'machine':'target'
 if(n.service)return n.service.kind==='socks'?'proxy':'service'
 if(n.id.startsWith('net:'))return 'bridge'
 if(n.kind==='访问来源'&&!n.machineId)return 'target'
 if(n.id.startsWith('if:')){
  if(n.id==='if:lo0')return 'loopback'
  if(n.id.startsWith('if:bridge'))return 'bridge'
  if(/wi-?fi/i.test(n.title))return 'wifi'
  return 'adapter'
 }
 return 'machine'
}
function nodeOS(n:Node){return n.machineId==='host'?'macos':n.data?.OS||n.data?.os||''}
function nodeState(n:Node){return n.machineId==='host'?'host':n.data?.State||n.data?.state||''}
function nodeStatus(n:Node){return n.machineId==='host'?'本机':machineStatus(nodeState(n))}
function edgeEndpoint(e:any,which:'from'|'to'){return graph.value.nodes.find(n=>n.id===e[which])!}
function edgeSelected(e:any){return focusedEdges.value.has(edgeKey(e))}
function nodeHighlighted(n:Node){return focusedEdges.value.size>0&&graph.value.edges.some(e=>edgeSelected(e)&&(e.from===n.id||e.to===n.id))}
async function refresh(){if(loading.value)return;loading.value=true;try{await refreshHost();await refreshServices();await Promise.all(state.vms.filter(v=>v.State==='running').map(async v=>{try{const value=await run('cli',['--vm',v.ID,'services','list'],true);snapshot.value[v.ID]={rows:value.services||[],at:Date.now(),error:''}}catch(e){const previous=snapshot.value[v.ID];snapshot.value[v.ID]={rows:previous?.rows||[],at:previous?.at||Date.now(),error:errorText(e)}}}))}finally{loading.value=false}}
async function log(){const row=active.value?.service;if(!row)return;if(row.ownerId!=='host'){state.vm=row.ownerId;await nextTick();await refreshServices()}state.selectedRule=serviceId(row);state.page=row.kind==='socks'?'socks':'forward'}
let timer:ReturnType<typeof setInterval>;onMounted(()=>{
 if(canvas.value)zoomControl=bindTopologyZoom(canvas.value,()=>({zoom:zoom.value,x:pan.x,y:pan.y}),camera=>{zoom.value=camera.zoom;pan.x=camera.x;pan.y=camera.y},pinch)
 void refresh();timer=setInterval(()=>{if(auto.value&&!state.pending)void refresh()},10000)
});onUnmounted(()=>{clearInterval(timer);zoomControl?.dispose()})
</script>
<template>
 <section class="topology-page flex h-full flex-col p-5">
  <div class="mb-4 flex shrink-0 items-center justify-between">
   <h1 class="flex items-center gap-2.5 text-lg font-semibold"><NetworkIcon class="size-5 text-violet-600"/>网络拓扑</h1>
   <div class="flex items-center gap-2"><label class="flex items-center gap-2 text-xs text-slate-600"><input v-model="auto" type="checkbox"/>每 10 秒刷新</label><Button variant="outline" :disabled="loading" @click="refresh"><RefreshCwIcon data-icon="inline-start"/>刷新拓扑</Button></div>
  </div>
  <div class="mb-3 flex flex-wrap items-center gap-2">
   <ToggleGroup :model-value="view" type="single" variant="outline" size="sm" @update:model-value="v=>{if(v)view=String(v)}"><ToggleGroupItem value="network">整体拓扑</ToggleGroupItem><ToggleGroupItem value="flow">服务流向</ToggleGroupItem><ToggleGroupItem value="access">访问权限</ToggleGroupItem></ToggleGroup>
   <div class="relative ml-auto w-44"><SearchIcon class="absolute left-2 top-2 size-4 text-slate-500"/><Input v-model="query" class="pl-8" :placeholder="view==='network'?'筛选虚拟机、IP…':'筛选入口、目标…'"/></div>
   <Button variant="outline" size="icon-sm" aria-label="缩小" :disabled="zoom<=MIN_ZOOM" @click="zoomControl?.setZoom(zoom-.1)"><MinusIcon/></Button><span class="w-10 text-center text-xs font-medium text-slate-700" title="触摸板双指捏合缩放 · 40%–200%">{{Math.round(zoom*100)}}%</span><Button variant="outline" size="icon-sm" aria-label="放大" :disabled="zoom>=MAX_ZOOM" @click="zoomControl?.setZoom(zoom+.1)"><PlusIcon/></Button><Button variant="outline" size="sm" @click="resetLayout"><RotateCcwIcon data-icon="inline-start"/>重置布局</Button>
  </div>
  <div class="topology-actions mb-3 flex shrink-0 flex-wrap items-center gap-2 rounded-md border bg-white px-3 py-2.5">
   <label for="topology-machine" class="text-xs font-medium text-slate-600">操作对象</label><div class="min-w-40 max-w-64 flex-1"><AppSelect id="topology-machine" :model-value="contextMachineId" :options="contextOptions" @update:model-value="setContext"/></div>
   <Button size="sm" @click="newService('forward')"><PlusIcon data-icon="inline-start"/>新建端口转发</Button>
   <Button size="sm" variant="outline" :disabled="contextMachineId==='host'" @click="newService('socks')"><GlobeIcon data-icon="inline-start"/>新建 SOCKS5 代理</Button>
   <Button size="sm" variant="outline" :disabled="contextMachineId==='host'" @click="openVPN"><ShieldCheckIcon data-icon="inline-start"/>配置 VPN</Button>
   <span class="basis-full text-[11px] text-slate-500">{{contextMachineId==='host'?'新建转发将在本地 Mac 运行；SOCKS5 与 VPN 需选择虚拟机。':`新建转发 / 代理将在 ${contextMachine?.Name||'所选虚拟机'} 内运行；VPN 使用该机器已有配置。`}} 点击图中机器可切换操作对象。</span>
  </div>
  <Alert v-if="state.error" variant="destructive" class="mb-3"><AlertDescription>{{state.error}}</AlertDescription></Alert>
  <div class="mb-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-slate-600"><template v-if="view==='network'"><span>灰色曲线：网络归属 · 蓝色箭头：端口转发 · 紫色箭头：SOCKS5 · 入口紧邻运行机器。</span><Button v-if="services.length" variant="link" size="sm" class="h-auto p-0 text-blue-700" @click="view='flow'">查看 {{services.length}} 条运行服务的流向</Button></template><template v-else><span class="inline-flex items-center gap-1.5"><i class="h-0.5 w-5 bg-blue-600"/>TCP 转发</span><span class="inline-flex items-center gap-1.5"><i class="h-0.5 w-5 bg-purple-600"/>SOCKS5 代理</span><span>虚线为来源规则，箭头为目标方向；配置不代表已建立连接。</span></template></div>
  <p class="mb-2 text-[11px] text-slate-500">触摸板双指捏合缩放 · 双指滑动浏览 · 拖动节点或空白区域调整位置</p>
  <div ref="canvas" :class="['topology-canvas min-h-[220px] flex-1 overflow-auto rounded-lg',{'is-panning':panning}]" tabindex="0" aria-label="网络拓扑画布，可双指捏合缩放、拖动节点或空白区域" @keydown.esc="clearFocus" @pointerdown="startPan" @pointermove="movePointer" @pointerup="endPointer" @pointercancel="endPointer" @click="backgroundClick">
   <div :style="{width:graph.width*zoom+'px',height:graph.height*zoom+'px',position:'relative'}">
    <div :style="{width:graph.width+'px',height:graph.height+'px',transform:`translate(${pan.x}px,${pan.y}px) scale(${zoom})`,transformOrigin:'top left',position:'relative'}">
     <div v-for="(label,i) in laneLabels" :key="label" class="topology-lane" :style="{left:COLUMNS[i]+'px'}"><span class="mr-2 text-slate-400">0{{i+1}}</span>{{label}}</div>
     <div v-if="!graph.nodes.length" class="absolute left-6 top-24 flex w-[920px] flex-col items-center gap-2 py-12 text-sm text-slate-600"><NetworkIcon class="mb-2 size-8 text-slate-400"/><strong class="font-medium">{{query?'没有匹配的转发或代理':'暂无运行中的转发或代理'}}</strong><span class="text-xs">{{query?'清空筛选查看全部服务。':'选择上方操作对象，再新建端口转发或 SOCKS5 代理；VPN 可从“配置 VPN”进入。'}}</span></div>
     <svg class="pointer-events-none absolute inset-0 h-full w-full overflow-visible">
      <defs><marker v-for="color in edgeColors" :id="`topology-arrow-${color.slice(1)}`" :key="color" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse"><path d="M 1 1 L 9 5 L 1 9 Z" :fill="color"/></marker></defs>
      <g v-for="e in graph.edges" :key="edgeKey(e)" role="button" tabindex="0" :aria-label="`${edgeEndpoint(e,'from').title} → ${edgeEndpoint(e,'to').title}：${e.label}`" class="topology-edge cursor-pointer" @click.stop="focusEdge(e)" @keydown.enter="focusEdge(e)">
       <title>{{edgeEndpoint(e,'from').title}} → {{edgeEndpoint(e,'to').title}}：{{e.label}}</title>
       <path :d="routes.get(edgeKey(e))?.path" fill="none" stroke="transparent" stroke-width="18"/>
       <path v-if="['forward','socks'].includes(e.kind)" :d="routes.get(edgeKey(e))?.path" fill="none" stroke="#fafafa" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>
       <path :d="routes.get(edgeKey(e))?.path" fill="none" :stroke="edgeColor(e)" :stroke-width="edgeSelected(e)?3:['forward','socks'].includes(e.kind)?2.4:1.8" :opacity="focusedEdges.size&&!edgeSelected(e) ? 0.8 : 1" :stroke-dasharray="edgeDashed(e)?'5 5':undefined" stroke-linejoin="round" :marker-end="['network','ownership'].includes(e.kind)?undefined:`url(#topology-arrow-${edgeColor(e).slice(1)})`"/>
       <text v-if="routes.get(edgeKey(e))?.text" :x="routes.get(edgeKey(e))!.text!.x" :y="routes.get(edgeKey(e))!.text!.y" :fill="edgeSelected(e)?'#c2410c':edgeColor(e)" font-size="11" font-weight="500" text-anchor="middle" paint-order="stroke" stroke="#fafafa" stroke-width="6" stroke-linejoin="round">{{e.label}}</text>
      </g>
     </svg>
     <button v-for="n in graph.nodes" :key="n.id" :style="{left:n.x+'px',top:n.y+'px'}" :class="['topology-node',{selected:selected===n.id,'service-card':!!n.service&&n.kind!=='实际目标','proxy-card':n.service?.kind==='socks'&&n.kind!=='实际目标','route-highlight':nodeHighlighted(n),'is-dragging':dragging===n.id}]" @pointerdown.stop="startNodeDrag($event,n)" @pointermove.stop="movePointer" @pointerup.stop="endPointer" @pointercancel.stop="endPointer" @click.stop="clickNode(n.id,$event)" @dblclick="n.service&&log()">
      <div class="node-heading">
       <MachineIcon v-if="nodeIcon(n)==='machine'" :os="nodeOS(n)" :name="n.title" tile/>
       <span v-else :class="['node-device-icon',nodeIcon(n)]" aria-hidden="true"><WifiIcon v-if="nodeIcon(n)==='wifi'"/><Repeat2Icon v-else-if="nodeIcon(n)==='loopback'"/><NetworkIcon v-else-if="nodeIcon(n)==='bridge'"/><EthernetPortIcon v-else-if="nodeIcon(n)==='adapter'"/><GlobeIcon v-else-if="['target','proxy'].includes(nodeIcon(n))"/><ArrowLeftRightIcon v-else/></span>
       <div class="min-w-0 flex-1"><div class="node-kind">{{n.kind}}<span v-if="nodeIcon(n)==='machine'"> · {{machineAppearance(nodeOS(n),n.title).label}}</span></div><div class="node-title" :title="n.title">{{n.title}}</div></div>
      </div>
      <div class="node-footer"><span class="node-sub" :title="n.sub">{{n.sub}}</span><span v-if="nodeIcon(n)==='machine'&&nodeState(n)" :class="['node-status',nodeState(n)]"><span/>{{nodeStatus(n)}}</span></div>
     </button>
    </div>
   </div>
  </div>
  <div v-if="focusedEdges.size" class="mt-3 flex shrink-0 items-center gap-3 rounded-md border border-orange-200 bg-orange-50 px-3 py-2 text-xs text-orange-900"><span v-if="selectedEdge">已高亮路线：{{edgeLabel}}</span><span v-else>已高亮关联路线：{{active?.title}} · {{focusedEdges.size}} 条</span><Button v-if="active?.vm&&!active.service" variant="outline" size="sm" class="ml-auto" @click="state.vm=active!.vm!;state.page='network'">管理网络</Button><Button variant="ghost" size="sm" class="ml-auto shrink-0 text-orange-900" @click="clearFocus">取消高亮</Button></div>
  <div v-if="active?.service" class="mt-2 flex shrink-0 flex-wrap items-center gap-3 text-xs"><span class="text-slate-600">{{active.service.kind==='socks'?'SOCKS5':'TCP 转发'}} · 运行于 {{active.service.ownerName}} · 允许来源 {{active.service.allow}}</span><Button variant="outline" size="sm" class="ml-auto" @click="log">查看规则与日志</Button><Button variant="outline" size="sm" @click="adjust">按此配置调整</Button></div>
  <p v-for="(s,id) in snapshot" v-show="s.error" :key="id" class="mt-2 text-xs text-destructive">{{id}}：服务读取失败，保留旧快照 {{s.error}}</p>
 </section>
</template>
<style scoped>
.topology-canvas{cursor:grab;touch-action:none;overflow-anchor:none;user-select:none;-webkit-user-select:none;border:1px solid #cbd5e1;background-color:#fafafa;background-image:radial-gradient(#d4d8df .7px,transparent .7px);background-size:20px 20px}
.topology-lane{position:absolute;top:22px;width:248px;border-bottom:1px solid #dce1e7;padding-bottom:10px;font-size:12px;font-weight:600;color:#334155;pointer-events:none}
.topology-node{cursor:grab;touch-action:none;user-select:none;-webkit-user-select:none;position:absolute;display:flex;flex-direction:column;justify-content:space-between;width:248px;height:112px;padding:13px;border:1px solid #c4cbd4;border-radius:8px;background:#fff;text-align:left;box-shadow:0 2px 5px #0f172a0d;transition:border-color .12s,box-shadow .12s}
.topology-canvas.is-panning,.topology-node.is-dragging{cursor:grabbing}
.topology-node.is-dragging{z-index:2;box-shadow:0 6px 18px #0f172a24}
.topology-node.selected{border-color:#ea580c;outline:2px solid #fed7aa;outline-offset:1px}
.topology-node:hover{border-color:#94a3b8;box-shadow:0 3px 8px #0f172a14}
.topology-node:focus-visible{border-color:#2563eb;outline:2px solid #bfdbfe;outline-offset:1px}
.topology-node.route-highlight{border-color:#ea580c;box-shadow:0 0 0 1px #fdba74}
.node-heading{display:flex;align-items:center;gap:11px;min-height:48px}
.node-kind{font-size:11px;line-height:16px;color:#475569;font-weight:500;margin-bottom:3px}
.node-title{display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;font-size:14px;line-height:19px;font-weight:600;color:#171717;overflow-wrap:anywhere}
.node-device-icon{display:flex;align-items:center;justify-content:center;flex-shrink:0;width:38px;height:38px;border:1px solid #dbe4ee;border-radius:8px;background:#eff6ff;color:#2563eb}
.node-device-icon :deep(svg){width:21px;height:21px;stroke-width:2}
.node-device-icon.bridge{color:#7c3aed;background:#f5f0ff;border-color:#e9ddff}
.node-device-icon.loopback{color:#475569;background:#f1f5f9;border-color:#dce3eb}
.node-device-icon.target{color:#c2410c;background:#fff4eb;border-color:#fed7aa}
.node-device-icon.proxy{color:#7c3aed;background:#f5f0ff;border-color:#e9ddff}
.topology-node.service-card{background:#f8fbff;border-color:#b8cff0}
.topology-node.proxy-card{background:#fcfaff;border-color:#d7c8f5}
.node-device-icon.service{color:#0369a1;background:#eef8ff;border-color:#cce8fb}
.node-footer{display:flex;align-items:center;gap:6px;min-height:20px;margin-top:5px}
.node-sub{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px;line-height:16px;color:#475569}
.node-status{display:inline-flex;align-items:center;gap:4px;flex-shrink:0;padding:1px 5px;border-radius:4px;background:#f1f5f9;color:#475569;font-size:11px;font-weight:500;line-height:17px}
.node-status>span{width:5px;height:5px;border-radius:50%;background:currentColor}
.node-status.running{color:#15803d;background:#dcfce7}
.node-status.suspended,.node-status.paused{color:#a16207;background:#fef3c7}
.node-status.host{color:#334155;background:#e9eef3}
.topology-edge:focus{outline:none}.topology-edge:focus-visible path:last-of-type{stroke:#ea580c;stroke-width:3}
.topology-edge{pointer-events:stroke}
@media(prefers-reduced-motion:reduce){.topology-node{transition:none}}
@media(max-height:740px){.topology-canvas{min-height:140px}.topology-page{padding-top:12px;padding-bottom:12px}}
</style>
