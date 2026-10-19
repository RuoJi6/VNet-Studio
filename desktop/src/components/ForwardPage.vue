<script setup lang="ts">
import {computed,ref,watch,onMounted,toRefs} from 'vue'
import {ArrowLeftRightIcon,ArrowRightIcon,PlusIcon,SearchIcon,RefreshCwIcon,CopyIcon,ExternalLinkIcon,PlayIcon,SquareIcon,Trash2Icon,InfoIcon,ChevronDownIcon,AlertCircleIcon} from '@lucide/vue'
import {Button} from '@/components/ui/button'
import {Input} from '@/components/ui/input'
import {Textarea} from '@/components/ui/textarea'
import {Field,FieldLabel,FieldGroup,FieldDescription} from '@/components/ui/field'
import {ToggleGroup,ToggleGroupItem} from '@/components/ui/toggle-group'
import {Tabs,TabsList,TabsTrigger,TabsContent} from '@/components/ui/tabs'
import {Empty,EmptyHeader,EmptyMedia,EmptyTitle,EmptyDescription} from '@/components/ui/empty'
import {Alert,AlertTitle,AlertDescription} from '@/components/ui/alert'
import {state,guest,guestArgs,guestReady,run,refreshServices,serviceId,addRules,changeService,refreshHost,type Service} from '@/lib/store'
import {errorText,openEntry,networkSettings,native} from '@/lib/bridge'
import {Checkbox} from '@/components/ui/checkbox'
import {forwardDrafts} from '@/lib/forwardState'
import AppSelect from './AppSelect.vue'
import ConnectionLogs from './ConnectionLogs.vue'
import VmPicker from './VmPicker.vue'
import {needsLocalNetworkHelp,loopbackTarget,replaceTargetIP} from '@/lib/forwardHelp'
const socks=computed(()=>state.page==='socks')
const draft=socks.value?forwardDrafts.socks:forwardDrafts.forward
const {location,allow,accessMode,gost}=toRefs(draft);const query=ref('');const scope=ref('all');const advanced=ref(false);const detail=ref('logs');const check=ref<any>(null)
const forms=draft.forms
const form=computed(()=>location.value==='host'?forms.host:forms.guest)
const addresses=ref<{value:string,label:string}[]>([])
watch(()=>state.vm,()=>{forms.guest.ip='';addresses.value=[]})
onMounted(()=>{if(!state.host)void refreshHost()})
const clients=computed(()=>[{value:'host',name:'本地 Mac',ips:[...new Set<string>((state.host?.interfaces||[]).flatMap((i:any)=>i.ips||[]))].filter(ip=>!ip.startsWith('127.'))},...state.vms.map(v=>({value:v.ID,name:v.Name,ips:(v.Network?.ipAddresses||[]).filter(i=>i.type==='ipv4').map(i=>i.ip)}))])
const effectiveAllow=computed(()=>accessMode.value==='all'?'0.0.0.0/0':accessMode.value==='custom'?allow.value:clients.value.filter(c=>draft.sources.includes(c.value)).flatMap(c=>c.ips).map(ip=>`${ip}/32`).join(','))
function selectSource(id:string,checked:boolean){draft.sources=accessMode.value==='single'?(checked?[id]:[]):checked?[...new Set([...draft.sources,id])]:draft.sources.filter(s=>s!==id)}
watch(accessMode,()=>{if(accessMode.value==='single')draft.sources=draft.sources.slice(0,1)})
const all=computed(()=>[...socks.value?[]:state.mac,...state.guest.filter(r=>r.kind===(socks.value?'socks':'forward'))])
const filtered=computed(()=>all.value.filter(r=>(scope.value==='all'||(scope.value==='host')===(r.location==='host'))&&`${r.ip}:${r.port} ${r.target}`.toLowerCase().includes(query.value.toLowerCase())))
const selected=computed(()=>all.value.find(r=>serviceId(r)===state.selectedRule))
const ready=computed(()=>!!form.value.ip&&(form.value.batch?!!form.value.text.trim():!!form.value.port&&(socks.value||!!form.value.target))&&(location.value==='host'||(guestReady()&&!!effectiveAllow.value)))
const targets=computed(()=>guest()?.Network?.ipAddresses?.filter(i=>i.type==='ipv4').map(i=>i.ip).join(', ')||'')
const loopbackWarning=computed(()=>location.value==='host'&&!socks.value&&!form.value.batch&&loopbackTarget(form.value.target))
const targetChoices=computed(()=>[...new Set((guest()?.Network?.ipAddresses||[]).filter(i=>i.type==='ipv4'&&privateIP(i.ip)).map(i=>i.ip))])
function useGuestTarget(ip:string){form.value.target=replaceTargetIP(form.value.target,ip);state.error=''}
async function readIPs(){try{const rows=await run('cli',guestArgs('addresses'));addresses.value=Array.isArray(rows)?rows.filter((r:any)=>privateIP(r.ip)).map((r:any)=>({value:r.ip,label:`${r.ip} · ${r.interface}`})):[]}catch{}}
async function submit(){try{const f=form.value;let rules;if(f.batch){rules=f.text.split('\n').filter(l=>l.trim()&&!l.trim().startsWith('#')).map(l=>{const w=l.trim().split(/\s+/);const expected=socks.value?1:2;if(w.length<expected||w.length>expected+(location.value==='host'?0:1))throw Error('批量格式不正确，请按每行的提示填写');return {ip:f.ip,port:w[0]!,target:socks.value?'':w[1]!,allow:w[expected]||effectiveAllow.value}})}else rules=[{ip:f.ip,port:f.port,target:f.target,allow:effectiveAllow.value}];await addRules(location.value,rules,socks.value?'socks':'forward',gost.value);state.selectedRule='';}catch(e){state.error=errorText(e)}}
function copyEntry(r:Service){void navigator.clipboard.writeText(socks.value?endpoint(r):`http://${endpoint(r)}`)}
function privateIP(ip:string){const n=ip.split('.').map(Number);return n.length===4&&(n[0]===10||n[0]===192&&n[1]===168||n[0]===172&&n[1]!>=16&&n[1]!<=31)}
function endpoint(r:Service){return `${r.ip}:${r.port}`}
async function operate(op:'start'|'stop'|'delete'|'check'){if(!selected.value)return;check.value=await changeService(selected.value,op);if(op==='delete')state.selectedRule=''}
function choose(row:Service){state.selectedRule=serviceId(row);check.value=null;state.error=''}
</script>
<template>
 <section class="flex h-full flex-col p-5">
  <div class="mb-4 flex shrink-0 items-center justify-between"><div class="flex items-center gap-2.5"><ArrowLeftRightIcon class="size-5"/><h1 class="text-lg font-semibold">{{socks?'SOCKS5 代理':'端口转发'}}</h1></div><span class="text-xs text-muted-foreground">访问入口 → 转发运行位置 → 实际服务</span></div>
  <div class="flex min-h-0 flex-1 overflow-hidden rounded-lg border bg-background">
   <aside class="flex w-[214px] shrink-0 flex-col border-r bg-muted/10">
    <div class="flex h-12 items-center justify-between border-b px-3"><span class="font-medium">转发规则</span><Button size="icon-xs" variant="ghost" aria-label="新建规则" @click="state.selectedRule='';state.error='' "><PlusIcon/></Button></div>
    <div class="flex flex-col gap-2 border-b p-2.5"><div class="relative"><SearchIcon class="pointer-events-none absolute left-2 top-2 size-4 text-muted-foreground"/><Input v-model="query" class="pl-8" placeholder="搜索入口、目标…" aria-label="搜索规则"/></div><ToggleGroup v-model="scope" type="single" variant="outline" size="sm" class="w-full"><ToggleGroupItem value="all" class="flex-1">全部</ToggleGroupItem><ToggleGroupItem value="host" :disabled="socks" class="flex-1">Mac</ToggleGroupItem><ToggleGroupItem value="guest" class="flex-1">虚拟机</ToggleGroupItem></ToggleGroup></div>
    <div class="min-h-0 flex-1 overflow-y-auto"><button v-for="r in filtered" :key="serviceId(r)" :class="['flex w-full flex-col gap-1.5 border-b px-3 py-3 text-left hover:bg-muted/60',selected&&serviceId(selected)===serviceId(r)?'bg-muted':'']" @click="choose(r)"><span class="text-[13px] font-medium">{{endpoint(r)}}</span><span class="flex items-center gap-1.5 text-xs text-muted-foreground"><span :class="['size-1.5 rounded-full',r.running?'bg-target':'bg-muted-foreground/40']"/>{{r.location==='host'?'本地 Mac':'虚拟机'}} · {{r.running?'运行中':'已停止'}}</span><span class="truncate text-[11px] text-muted-foreground">→ {{r.kind==='socks'?'客户端指定目标':r.target}}</span></button>
     <Empty v-if="!filtered.length" class="h-full min-h-[230px] p-4"><EmptyHeader><EmptyMedia><ArrowLeftRightIcon class="size-7 text-muted-foreground/35"/></EmptyMedia><EmptyTitle class="text-sm font-normal text-muted-foreground">{{all.length?'没有匹配的规则':'暂无转发规则'}}</EmptyTitle><EmptyDescription class="text-xs">点击 ＋ 创建一条规则</EmptyDescription></EmptyHeader></Empty>
    </div><div class="flex flex-col gap-2 border-t p-3"><span class="text-[11px] text-muted-foreground">{{all.length}} 条规则 · {{all.filter(r=>r.running).length}} 条运行中</span><Button variant="outline" class="w-full" :disabled="!!state.pending" @click="refreshServices"><RefreshCwIcon data-icon="inline-start"/>刷新规则</Button></div>
   </aside>
   <div class="flex min-w-0 flex-1 flex-col">
    <div class="min-h-0 flex-1 overflow-y-auto p-6">
     <Alert v-if="state.backendOld" class="mb-4"><InfoIcon/><AlertTitle>Mac 转发后台仍是旧版本</AlertTitle><AlertDescription>已有规则继续运行。停止所有 Mac 规则后，可重启后台以加载新版。<Button variant="outline" :disabled="!native||!!state.pending||state.mac.some(r=>r.running)" @click="run('cli',['host-forward','restart-backend']).then(()=>refreshServices()).catch(()=>{})">重启空闲后台</Button></AlertDescription></Alert>
     <Alert v-if="state.error" variant="destructive" class="mb-5"><AlertCircleIcon/><AlertTitle>操作未完成</AlertTitle><AlertDescription class="break-words">{{state.error}}<Button v-if="needsLocalNetworkHelp(state.error)" variant="outline" size="sm" @click="networkSettings">打开 Mac 本地网络权限</Button></AlertDescription></Alert>
     <Alert v-for="(e,k) in state.serviceErrors" v-show="e" :key="k" variant="destructive" class="mb-4"><AlertDescription>状态读取失败，保留上次快照：{{e}}</AlertDescription></Alert>
     <template v-if="!selected">
      <div class="mb-6 flex items-center gap-3"><h2 class="font-medium">新建规则</h2><span class="text-xs text-muted-foreground">端口和服务地址由你填写</span></div>
      <div class="grid grid-cols-[minmax(0,1fr)_24px_minmax(0,1fr)] items-start gap-5">
       <FieldGroup><h3 class="mb-1 font-medium text-entry">访问入口</h3>
        <Field><FieldLabel for="location">转发运行位置</FieldLabel><AppSelect id="location" v-model="location" :options="socks?[{value:'guest',label:'所选虚拟机'}]:[{value:'host',label:'本地 Mac'},{value:'guest',label:'所选虚拟机'}]"/></Field>
        <Field v-if="location==='guest'"><FieldLabel for="runner">运行服务的虚拟机</FieldLabel><VmPicker id="runner"/></Field>
        <Field><FieldLabel for="listen-ip">{{location==='host'?'Mac 监听 IP':'虚拟机监听 IP'}}</FieldLabel><Input id="listen-ip" v-model="form.ip" :placeholder="location==='host'?'127.0.0.1':'填写虚拟机的网卡 IPv4'"/><div v-if="location==='guest'" class="flex items-center gap-2"><AppSelect v-model="forms.guest.ip" :options="addresses.length?addresses:(guest()?.Network?.ipAddresses||[]).filter(i=>i.type==='ipv4'&&privateIP(i.ip)).map(i=>({value:i.ip,label:i.ip}))" placeholder="选择网卡 IP"/><Button variant="outline" :disabled="!guestReady()||!!state.pending" @click="readIPs">读取</Button></div></Field>
        <Field v-if="!form.batch"><FieldLabel for="listen-port">{{location==='host'?'Mac 本地入口端口':'虚拟机入口端口'}}</FieldLabel><Input id="listen-port" v-model="form.port" inputmode="numeric" placeholder="填写未占用的端口"/><FieldDescription>浏览器访问这个 IP 和端口。</FieldDescription></Field>
       </FieldGroup>
       <div class="flex justify-center pt-[70px]"><ArrowRightIcon class="size-5 text-muted-foreground"/></div>
       <FieldGroup><h3 class="mb-1 font-medium text-target">实际目标</h3>
        <Field><FieldLabel for="target-vm">{{location==='host'?'关联虚拟机（可选）':'当前虚拟机'}}</FieldLabel><VmPicker id="target-vm" :optional="location==='host'"/><FieldDescription v-if="targets">网卡 IP：{{targets}}</FieldDescription></Field>
        <Field v-if="!socks&&!form.batch"><FieldLabel for="target">实际服务 IPv4:端口</FieldLabel><Input id="target" v-model="form.target" placeholder="填写目标 IP:服务端口"/><FieldDescription>填写服务实际所在机器的 IP 和端口。</FieldDescription></Field>
        <Alert v-if="loopbackWarning" class="border-amber-300 bg-amber-50 text-amber-950"><InfoIcon/><AlertTitle>当前目标是本地 Mac{{guest()?'，不是关联虚拟机':''}}</AlertTitle><AlertDescription class="text-amber-900"><p>127.0.0.0/8 属于 Mac 自己。{{guest()?`选择 ${guest()!.Name} 只建立关联，不会改变目标地址。访问其服务请使用网卡 IP，端口保持为实际服务端口。`:'访问虚拟机服务时，请填写虚拟机网卡 IP 和实际服务端口。'}}</p><Button v-for="ip in targetChoices" :key="ip" variant="outline" size="sm" class="h-auto min-h-8 whitespace-normal text-left" @click="useGuestTarget(ip)">改用虚拟机地址 {{replaceTargetIP(form.target,ip)}}</Button></AlertDescription></Alert>
        <p v-if="socks" class="subtle">目标由 SOCKS5 客户端在连接时指定。支持 TCP CONNECT，不含 UDP。</p>
        <p class="subtle">{{location==='host'?'由本地 Mac 连接目标。目标 127.0.0.1 指 Mac 自己。':'由所选虚拟机连接目标。目标 127.0.0.1 指这台虚拟机自己。'}}</p>
       </FieldGroup>
      </div>
      <div class="mt-7 flex flex-col gap-5 border-t pt-5">
       <div class="flex items-center gap-4"><span class="panel-label">录入方式</span><ToggleGroup :model-value="form.batch?'batch':'single'" type="single" variant="outline" size="sm" @update:model-value="v=>{if(v)form.batch=v==='batch'}"><ToggleGroupItem value="single">单条规则</ToggleGroupItem><ToggleGroupItem value="batch">批量录入</ToggleGroupItem></ToggleGroup></div>
       <Field v-if="form.batch"><FieldLabel for="batch">批量规则</FieldLabel><Textarea id="batch" v-model="form.text" class="min-h-28" placeholder="每行填写一条规则"/><FieldDescription>{{socks?'代理端口 [允许来源网段]':location==='host'?'入口端口 目标IPv4:端口':'入口端口 目标IPv4:端口 [允许来源网段]'}}</FieldDescription></Field>
       <div v-if="location==='host'" class="flex items-center gap-4"><span class="panel-label">访问范围</span><span class="text-sm text-muted-foreground">仅此 Mac · 回环地址 127.0.0.0/8</span></div>
       <Field v-else><FieldLabel for="allow-mode">允许谁使用入口</FieldLabel><div class="flex items-center gap-2"><AppSelect id="allow-mode" v-model="accessMode" :options="[{value:'single',label:'选择一台来源主机'},{value:'multiple',label:'选择多台来源主机'},{value:'custom',label:'自定义 IP / 网段'},{value:'all',label:'全部 IPv4 来源'}]"/><Button variant="outline" :disabled="!!state.pending" @click="refreshHost">刷新来源</Button></div><Input v-if="accessMode==='custom'" v-model="allow" placeholder="IPv4/CIDR，多个来源用逗号分隔"/><div v-if="accessMode==='single'||accessMode==='multiple'" class="flex max-h-36 flex-col gap-3 overflow-auto rounded-md border p-3"><label v-for="c in clients" :key="c.value" class="flex items-center gap-2 text-xs"><Checkbox :model-value="draft.sources.includes(c.value)" :disabled="!c.ips.length" @update:model-value="v=>selectSource(c.value,v===true)"/><span>{{c.name}}<span class="ml-2 text-muted-foreground">{{c.ips.join(', ')||'暂无 IPv4'}}</span></span></label></div><span v-if="effectiveAllow" class="break-words text-xs text-muted-foreground">允许来源：{{effectiveAllow}}</span><FieldDescription>按实际源 IP 判断，IP 变化后需更新。</FieldDescription></Field>
       <Field v-if="socks&&guest()?.OS?.startsWith('win')"><FieldLabel>虚拟机内 GOST 路径</FieldLabel><Input v-model="gost" placeholder="gost.exe 或完整路径"/></Field>
       <div class="flex flex-col gap-1.5 rounded-md bg-muted/45 px-4 py-3"><span class="text-xs text-muted-foreground">连接路径</span><span class="break-words text-[13px]">{{form.ip||'待填写 IP'}}:{{form.port||'待填写端口'}} → {{location==='host'?'本地 Mac':'所选虚拟机'}} → {{socks?'客户端指定目标':form.target||'待填写实际目标'}}</span></div>
       <details class="text-xs text-muted-foreground"><summary class="flex items-center gap-1.5"><ChevronDownIcon class="size-3"/>连接说明与网络权限</summary><div class="flex flex-col gap-2 pt-3 leading-6"><p>停止保留配置；删除会停止服务并移除规则。不会自动开启任何规则。</p><p v-if="location==='host'">Mac 到目标使用当前路由，仍可能受 VPN/TUN 影响。首次询问本地网络权限时，请允许 VNet Studio。</p><Button v-if="location==='host'" variant="outline" class="w-fit" @click="networkSettings">打开 Mac 本地网络权限</Button></div></details>
      </div>
     </template>
     <template v-else>
      <div class="mb-6 flex items-center gap-3"><h2 class="font-medium">规则详情</h2><span :class="['flex items-center gap-1.5 text-xs',selected.running?'text-target':'text-muted-foreground']"><span class="size-1.5 rounded-full bg-current"/>{{selected.running?'运行中':'已停止'}}</span></div>
      <div class="grid grid-cols-[1fr_24px_1fr] gap-5"><div class="flex flex-col gap-3"><h3 class="font-medium text-entry">访问入口</h3><span class="font-medium">{{endpoint(selected)}}</span><span class="subtle">运行于：{{selected.location==='host'?'本地 Mac':guest()?.Name||'所选虚拟机'}}</span></div><ArrowRightIcon class="mt-9 size-5 text-muted-foreground"/><div class="flex flex-col gap-3"><h3 class="font-medium text-target">实际目标</h3><span class="font-medium">{{selected.target||'由客户端指定'}}</span><span class="subtle">{{selected.location==='host'?'由 Mac 发起目标连接':'由所选虚拟机发起目标连接'}}</span></div></div>
      <div class="my-5 flex items-center gap-2"><Button variant="outline" @click="copyEntry(selected)"><CopyIcon data-icon="inline-start"/>复制入口</Button><Button v-if="!socks" variant="outline" :disabled="!selected.running" @click="openEntry(`http://${endpoint(selected)}`)"><ExternalLinkIcon data-icon="inline-start"/>打开入口</Button><Button v-if="selected.location==='host'" variant="outline" :disabled="!!state.pending" @click="operate('check')">检查链路</Button></div>
      <Alert v-if="check?.target_tcp_ok!==undefined" :variant="check.target_tcp_ok?'default':'destructive'" class="mb-4"><InfoIcon/><AlertTitle>{{check.target_tcp_ok?'Mac 到实际目标 TCP 连接成功':'Mac 到实际目标连接失败'}}</AlertTitle><AlertDescription><pre>{{check.route||check.error}}</pre></AlertDescription></Alert>
      <Tabs v-model="detail" class="mt-6 border-t pt-4"><TabsList><TabsTrigger value="logs">连接日志</TabsTrigger><TabsTrigger value="info">规则信息</TabsTrigger></TabsList><TabsContent value="logs"><ConnectionLogs :key="`${selected.location}:${state.vm}:${serviceId(selected)}`" :events="selected.events"/></TabsContent><TabsContent value="info"><dl class="mt-4 grid grid-cols-[110px_1fr] gap-4 text-sm"><dt class="text-muted-foreground">允许来源</dt><dd>{{selected.allow}}</dd><dt class="text-muted-foreground">当前连接</dt><dd>{{selected.stats?.active??'—'}}</dd><dt class="text-muted-foreground">累计发送 / 接收</dt><dd>{{selected.stats?.tx??'—'}} / {{selected.stats?.rx??'—'}} B</dd></dl></TabsContent></Tabs>
     </template>
    </div>
    <footer class="flex min-h-16 shrink-0 items-center gap-3 border-t bg-muted/20 px-6 py-3"><span class="mr-auto text-xs text-muted-foreground">{{selected?'每条规则独立管理':ready?'配置已填写，开启前会检查入口和目标':'填写完整配置后即可开启'}}</span><Button v-if="selected" variant="outline" :disabled="!!state.pending||!native||!selected.managed" @click="operate('delete')"><Trash2Icon data-icon="inline-start"/>删除规则</Button><Button v-if="selected" :disabled="!!state.pending||!native||!selected.managed" @click="operate(selected.running?'stop':'start')"><SquareIcon v-if="selected.running" data-icon="inline-start"/><PlayIcon v-else data-icon="inline-start"/>{{selected.running?'停止转发':'开启转发'}}</Button><Button v-else :disabled="!ready||!!state.pending||!native" @click="submit"><PlusIcon data-icon="inline-start"/>{{form.batch?'批量添加并开启':'添加并开启'}}</Button></footer>
   </div>
  </div>
 </section>
</template>
