<script setup lang="ts">
import {onMounted,onUnmounted,watch} from 'vue'
import {ArrowLeftRightIcon,NetworkIcon,MonitorIcon,RouteIcon,ShieldCheckIcon,ContainerIcon,ScrollTextIcon,GlobeIcon,LayoutDashboardIcon,PlusIcon,RefreshCwIcon,ChevronRightIcon,LoaderCircleIcon,PanelLeftIcon} from '@lucide/vue'
import {Button} from '@/components/ui/button'
import {state,refreshVms,refreshServices,refreshHost,type Page} from '@/lib/store'
import {native} from '@/lib/bridge'
import MachineIcon from '@/components/MachineIcon.vue'
import ForwardPage from '@/components/ForwardPage.vue'
import ManagementPages from '@/components/ManagementPages.vue'
import TopologyPage from '@/components/TopologyPage.vue'
const pageColors:Record<string,string>={overview:'text-sky-600',topology:'text-violet-600',network:'text-cyan-700',routes:'text-amber-600',forward:'text-blue-600',socks:'text-purple-600',vpn:'text-emerald-600',containers:'text-blue-700',services:'text-slate-700'}
const pages:{key:Page;name:string;icon:any}[]=[{key:'overview',name:'网络概览',icon:LayoutDashboardIcon},{key:'topology',name:'网络拓扑',icon:NetworkIcon},{key:'network',name:'网卡与设备',icon:MonitorIcon},{key:'routes',name:'默认出口与路由',icon:RouteIcon},{key:'forward',name:'端口转发',icon:ArrowLeftRightIcon},{key:'socks',name:'SOCKS5',icon:GlobeIcon},{key:'vpn',name:'VPN',icon:ShieldCheckIcon},{key:'containers',name:'容器',icon:ContainerIcon},{key:'services',name:'运行与日志',icon:ScrollTextIcon}]
function navigate(page:Page){state.page=page;state.error='';if(['forward','socks','services','topology'].includes(page))void refreshServices()}
watch(()=>state.vm,()=>{state.guest=[];state.selectedRule='';void refreshServices()})
let timer:ReturnType<typeof setInterval>
onMounted(async()=>{await refreshVms();await refreshServices();timer=setInterval(()=>{if(!state.pending&&['forward','socks','services','topology'].includes(state.page))void refreshServices()},5000)})
onUnmounted(()=>clearInterval(timer))
</script>
<template>
 <div class="flex h-full flex-col bg-background">
  <header class="flex h-12 shrink-0 items-center gap-1 border-b bg-muted/40 px-4">
   <div class="mr-5 flex items-center gap-2.5 font-semibold"><NetworkIcon class="size-5"/>VNet Studio</div>
   <Button variant="ghost" @click="state.selectedRule='';navigate('forward')"><PlusIcon data-icon="inline-start"/>新建转发</Button>
   <Button variant="ghost" @click="navigate('topology')"><NetworkIcon data-icon="inline-start"/>网络拓扑</Button>
   <Button variant="ghost" @click="navigate('services')"><ScrollTextIcon data-icon="inline-start"/>运行与日志</Button>
   <div class="ml-auto flex items-center gap-4"><span class="text-xs text-muted-foreground">{{state.vms.filter(v=>v.State==='running').length}} 台运行中</span><Button variant="outline" :disabled="!!state.pending" @click="refreshVms"><RefreshCwIcon data-icon="inline-start"/>刷新</Button></div>
  </header>
  <div class="flex min-h-0 flex-1">
   <aside class="flex w-[228px] shrink-0 flex-col border-r bg-muted/20 max-[1100px]:w-[196px]">
    <div class="flex h-10 items-center justify-between border-b px-3"><span class="text-xs font-medium">本机与虚拟机</span><Button variant="ghost" size="icon-xs" aria-label="刷新虚拟机列表" @click="refreshVms"><RefreshCwIcon/></Button></div>
    <div class="border-b p-2"><Button variant="ghost" class="w-full justify-start" @click="state.vm='';navigate('forward')"><MachineIcon os="macos"/>本地 Mac<span class="ml-auto size-1.5 rounded-full bg-target"/></Button></div>
    <div class="min-h-0 flex-1 overflow-auto py-2">
     <div class="px-3 pb-2 text-[11px] text-muted-foreground">PARALLELS DESKTOP</div>
     <button v-for="v in state.vms" :key="v.ID" :class="['flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] hover:bg-muted',state.vm===v.ID?'bg-blue-100/80 font-medium':'']" @click="state.vm=v.ID">
      <ChevronRightIcon class="size-3 text-muted-foreground"/><MachineIcon :os="v.OS" :name="v.Name"/><span class="min-w-0 flex-1 truncate" :title="v.Name">{{v.Name}}</span><span :class="['size-1.5 shrink-0 rounded-full',v.State==='running'?'bg-emerald-500':'bg-slate-400']"/>
     </button>
     <p v-if="!state.vms.length" class="p-4 text-xs text-muted-foreground">{{state.loaded?'暂无虚拟机':'正在读取虚拟机…'}}</p>
    </div>
    <div class="border-t p-3 text-[11px] leading-5 text-muted-foreground">本地管理 · 不会自动启动虚拟机<br/>Windows / Linux / macOS</div>
   </aside>
   <div class="flex min-w-0 flex-1 flex-col">
    <nav class="flex h-10 shrink-0 overflow-x-auto border-b bg-muted/40" aria-label="功能导航"><button v-for="p in pages" :key="p.key" :class="['flex shrink-0 items-center gap-2 border-r px-3 text-[13px]',state.page===p.key?'border-b-2 border-b-primary bg-background font-medium':'text-muted-foreground hover:bg-muted']" @click="navigate(p.key)"><component :is="p.icon" :class="['size-4',pageColors[p.key]]"/>{{p.name}}</button></nav>
    <main class="min-h-0 flex-1"><ForwardPage v-if="state.page==='forward'||state.page==='socks'" :key="state.page"/><TopologyPage v-else-if="state.page==='topology'"/><ManagementPages v-else/></main>
   </div>
  </div>
  <footer class="flex h-7 shrink-0 items-center gap-2 border-t bg-muted/20 px-3 text-[11px] text-muted-foreground"><LoaderCircleIcon v-if="state.pending" class="size-3 animate-spin"/><PanelLeftIcon v-else class="size-3"/><span>{{state.pending?'正在读取或执行…':state.status}}</span><span v-if="!native" class="ml-3">浏览器预览仅支持读取，写操作请在桌面应用中执行</span><span class="ml-auto">Tauri 2 · v0.10.6</span></footer>
 </div>
</template>
