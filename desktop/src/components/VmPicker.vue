<script setup lang="ts">
import {computed} from 'vue'
import AppSelect from './AppSelect.vue'
import {machineAppearance} from '@/lib/machine'
import {state} from '@/lib/store'
const props=defineProps<{id?:string;optional?:boolean}>()
const options=computed(()=>[...(props.optional?[{value:'__none',label:'不关联虚拟机'}]:[]),...state.vms.map(v=>({value:v.ID,icon:machineAppearance(v.OS,v.Name).src,label:`${v.Name} · ${v.State==='running'?'运行中':v.State==='suspended'?'已挂起':'已停止'}`}))])
function choose(value:string){state.vm=value==='__none'?'':value}
</script>
<template><AppSelect :id="id" :model-value="state.vm" :options="options" placeholder="选择虚拟机" @update:model-value="choose"/></template>
