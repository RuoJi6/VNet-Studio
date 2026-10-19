import apple from '@/assets/machines/apple.svg'
import ubuntu from '@/assets/machines/ubuntu.svg'
import debian from '@/assets/machines/debian.svg'
import kali from '@/assets/machines/kalilinux.svg'
import windows from '@/assets/machines/windows.svg'
import linux from '@/assets/machines/linux.svg'

const machines = {
  macos: {label:'macOS',src:apple,surface:'#f1f3f5'},
  ubuntu: {label:'Ubuntu',src:ubuntu,surface:'#fff0e9'},
  debian: {label:'Debian',src:debian,surface:'#fff0f3'},
  kali: {label:'Kali Linux',src:kali,surface:'#edf5ff'},
  windows: {label:'Windows',src:windows,surface:'#edf6ff'},
  linux: {label:'Linux',src:linux,surface:'#fff8e6'},
  unknown: {label:'未知系统',src:'',surface:'#f1f5f9'},
}
export function machineAppearance(os='',name='') {
  const classify=(value:string)=>{
    const s=value.toLowerCase()
    if(/mac|osx|darwin/.test(s))return 'macos'
    if(/ubuntu/.test(s))return 'ubuntu'
    if(/debian/.test(s))return 'debian'
    if(/kali/.test(s))return 'kali'
    if(/windows|^win(?:[-_\s\d]|$)/.test(s))return 'windows'
  }
  const key=classify(os)||classify(name)||(/linux|unix/i.test(os+' '+name)?'linux':'unknown')
  return {...machines[key],key}
}
export function machineStatus(state?:string) {
  return ({running:'运行中',suspended:'已挂起',paused:'已暂停',stopped:'已停止'} as Record<string,string>)[state||'']||'状态未知'
}
