import { invoke } from '@tauri-apps/api/core'
export const native = '__TAURI_INTERNALS__' in window
export async function execute(purpose:string,args:string[]=[]){
 if(native)return invoke<{text:string,data:any}>('execute',{purpose,args})
 const res=await fetch('/__vnet/read',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({purpose,args})})
 const value=await res.json();if(!res.ok)throw Error(value.error||'读取失败');return value
}
export async function openEntry(url:string){if(native)await invoke('open_entry',{url});else window.open(url,'_blank','noopener')}
export async function networkSettings(){if(native)await invoke('open_network_settings');}
export function errorText(e:unknown){const text=String(e instanceof Error?e.message:e);const match=text.match(/"detail"\s*:\s*"((?:[^"\\]|\\.)*)"/);if(match){try{return JSON.parse(`"${match[1]}"`)}catch{}}return text.replace(/^Error:\s*/,'')}
