<script setup lang="ts">
import {computed,ref,shallowRef,watch} from 'vue'
import {ChevronLeftIcon,ChevronRightIcon,ChevronsLeftIcon,ChevronsRightIcon} from '@lucide/vue'
import {Button} from '@/components/ui/button'
import {Empty,EmptyHeader,EmptyTitle,EmptyDescription} from '@/components/ui/empty'
import AppSelect from './AppSelect.vue'

const props=defineProps<{events?:Record<string,any>[]}>()
const page=ref(1)
const size=ref('10')
// Keep older pages stable while the service refreshes every five seconds.
const snapshot=shallowRef<Record<string,any>[]>([])
const updated=ref(false)
watch(()=>props.events,events=>{
  const next=events||[]
  if(page.value===1){snapshot.value=[...next];updated.value=false}
  else updated.value=JSON.stringify(next)!==JSON.stringify(snapshot.value)
},{immediate:true,deep:true})
const total=computed(()=>snapshot.value.length)
const pages=computed(()=>Math.max(1,Math.ceil(total.value/Number(size.value))))
const start=computed(()=>(page.value-1)*Number(size.value))
const rows=computed(()=>snapshot.value.slice().reverse().slice(start.value,start.value+Number(size.value)))
function latest(){page.value=1;snapshot.value=[...(props.events||[])];updated.value=false}
function go(next:number){page.value=Math.max(1,Math.min(pages.value,next));if(page.value===1)latest()}
watch(size,latest)
function eventTime(time:string){const seconds=Number(time?.split(' ')[0]);return Number.isFinite(seconds)?new Date(seconds*1000).toLocaleString('zh-CN',{hour12:false}):time}
function eventName(event:string){return ({started:'转发已开启',stopped:'转发已停止',accepted:'连接已接受',connected:'连接已建立',closed:'连接已结束',connect_error:'连接目标失败',failed:'连接失败',start_error:'开启失败',rejected:'来源被拒绝'} as Record<string,string>)[event]||event}
</script>

<template>
 <div class="min-w-0 pt-3">
  <nav v-if="total" aria-label="连接日志分页" class="sticky top-0 flex flex-wrap items-center gap-3 border-b bg-background py-3">
   <span class="text-xs text-muted-foreground" role="status" aria-live="polite">共 {{total}} 条 · 第 {{start+1}}–{{Math.min(start+Number(size),total)}} 条</span>
   <div class="ml-auto flex flex-wrap items-center gap-2">
    <label for="log-page-size" class="text-xs text-muted-foreground">每页</label>
    <div class="w-[88px]"><AppSelect id="log-page-size" v-model="size" :options="[{value:'10',label:'10 条'},{value:'20',label:'20 条'},{value:'50',label:'50 条'}]"/></div>
    <Button variant="outline" size="icon-sm" aria-label="日志首页" :disabled="page===1" @click="go(1)"><ChevronsLeftIcon/></Button>
    <Button variant="outline" size="icon-sm" aria-label="日志上一页" :disabled="page===1" @click="go(page-1)"><ChevronLeftIcon/></Button>
    <span class="min-w-16 text-center text-xs" aria-label="日志页码">{{page}} / {{pages}}</span>
    <Button variant="outline" size="icon-sm" aria-label="日志下一页" :disabled="page===pages" @click="go(page+1)"><ChevronRightIcon/></Button>
    <Button variant="outline" size="icon-sm" aria-label="日志末页" :disabled="page===pages" @click="go(pages)"><ChevronsRightIcon/></Button>
   </div>
  </nav>
  <div v-if="page>1" class="flex flex-wrap items-center gap-2 py-2 text-xs text-muted-foreground"><span>{{updated?'日志有更新，当前页保留浏览快照':'正在浏览历史日志，自动刷新不会打乱当前页'}}</span><Button variant="ghost" size="sm" @click="latest">回到最新</Button></div>
  <ol v-if="total" aria-label="连接日志记录" class="flex flex-col gap-3 pt-3">
   <li v-for="(e,i) in rows" :key="start+i" class="min-w-0 break-words border-b pb-3 text-xs">
    <div class="mb-1 flex flex-wrap gap-x-3 gap-y-1"><span class="text-muted-foreground">{{eventTime(e.time)}}</span><span>{{eventName(e.event)}}</span></div>
    <div v-if="e.peer||e.target">{{e.peer||'入口'}} → {{e.target||'连接已关闭'}}</div>
    <p v-if="e.tx!==undefined" class="mt-1 text-muted-foreground">发送 {{e.tx}} B · 接收 {{e.rx}} B · {{e.duration_ms}} ms</p>
    <p v-if="e.error" class="mt-1 text-destructive">{{e.error}}</p>
   </li>
  </ol>
  <Empty v-else class="min-h-48"><EmptyHeader><EmptyTitle class="text-sm">暂无连接记录</EmptyTitle><EmptyDescription>客户端连接入口后，记录将在这里显示</EmptyDescription></EmptyHeader></Empty>
 </div>
</template>
