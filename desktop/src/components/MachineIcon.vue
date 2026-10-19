<script setup lang="ts">
import {computed} from 'vue'
import {MonitorIcon} from '@lucide/vue'
import {machineAppearance} from '@/lib/machine'
const props=withDefaults(defineProps<{os?:string;name?:string;tile?:boolean}>(),{os:'',name:'',tile:false})
const machine=computed(()=>machineAppearance(props.os,props.name))
</script>
<template>
  <span :class="['machine-icon',tile?'machine-icon-tile':'']" :style="tile?{backgroundColor:machine.surface}:undefined" :title="machine.label" :data-machine="machine.key" aria-hidden="true">
    <img v-if="machine.src" :src="machine.src" alt="" draggable="false"/>
    <MonitorIcon v-else/>
  </span>
</template>
<style scoped>
.machine-icon{display:inline-flex;align-items:center;justify-content:center;width:20px;height:20px;flex-shrink:0;vertical-align:middle}
.machine-icon img,.machine-icon :deep(svg){display:block;width:100%;height:100%;object-fit:contain}
.machine-icon-tile{width:38px;height:38px;border-radius:8px;border:1px solid #e2e8f0;padding:8px}
</style>
