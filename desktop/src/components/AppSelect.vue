<script setup lang="ts">
import {computed} from 'vue'
import {Select,SelectTrigger,SelectValue,SelectContent,SelectItem,SelectGroup} from '@/components/ui/select'
const props=defineProps<{modelValue:string;options:{value:string;label:string;disabled?:boolean;icon?:string}[];placeholder?:string;disabled?:boolean;id?:string}>()
const selected=computed(()=>props.options.find(o=>o.value===props.modelValue))
const emit=defineEmits<{(e:'update:modelValue',value:string):void}>()
</script>
<template><Select :model-value="modelValue||undefined" :disabled="disabled" @update:model-value="v=>emit('update:modelValue',String(v))"><SelectTrigger :id="id" class="w-full"><SelectValue v-if="!selected?.icon" :placeholder="placeholder||'请选择'"/><SelectValue v-else><span class="flex min-w-0 items-center gap-2"><img :src="selected.icon" alt="" class="size-4 shrink-0"/><span class="truncate">{{selected.label}}</span></span></SelectValue></SelectTrigger><SelectContent><SelectGroup><SelectItem v-for="o in options" :key="o.value" :value="o.value" :disabled="o.disabled"><span class="flex items-center gap-2"><img v-if="o.icon" :src="o.icon" alt="" class="size-4 shrink-0"/>{{o.label}}</span></SelectItem></SelectGroup></SelectContent></Select></template>
