// Look only at the reported failure, not numbers in targets or remedy text.
// This also handles older backends that appended error-65 advice to every error.
export function needsLocalNetworkHelp(error:string){
 const failure=error.split('连接失败：')[1]?.split('。')[0]||error.split('。')[0]||''
 return /No route to host|(?:os error|错误)\s*65\b/i.test(failure)
}
export function loopbackTarget(target:string){return /^127(?:\.\d{1,3}){3}:\d+$/.test(target.trim())}
export function replaceTargetIP(target:string,ip:string){
 const port=target.trim().match(/:(\d+)$/)?.[1]
 return port?`${ip}:${port}`:''
}
