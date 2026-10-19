<script setup lang="ts">
import {Table,TableHeader,TableRow,TableHead,TableBody,TableCell} from '@/components/ui/table'
defineProps<{columns:{key:string,label:string}[];rows:any[];clickable?:boolean}>()
defineEmits<{(e:'choose',row:any):void}>()
function text(v:any){return v===null||v===undefined||v===''?'—':typeof v==='object'?JSON.stringify(v):String(v)}
</script>
<template><Table><TableHeader><TableRow><TableHead v-for="c in columns" :key="c.key" class="h-10 whitespace-nowrap">{{c.label}}</TableHead><TableHead v-if="$slots.actions" class="w-32">操作</TableHead></TableRow></TableHeader><TableBody><TableRow v-for="(r,i) in rows" :key="r.ID||r.id||i" :class="clickable?'cursor-pointer':''" @click="clickable&&$emit('choose',r)"><TableCell v-for="c in columns" :key="c.key" class="py-3">{{text(r[c.key])}}</TableCell><TableCell v-if="$slots.actions"><slot name="actions" :row="r"/></TableCell></TableRow><TableRow v-if="!rows.length"><TableCell :colspan="columns.length+($slots.actions?1:0)" class="h-28 text-center text-muted-foreground">暂无数据</TableCell></TableRow></TableBody></Table></template>
