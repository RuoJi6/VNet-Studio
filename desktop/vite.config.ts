import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'
import {execFile} from 'node:child_process'
import {promisify} from 'node:util'
const exec=promisify(execFile)
// Read-only development bridge. Production has no HTTP server; writes use Tauri IPC.
const readBridge:Plugin={name:'vnet-read-only-preview',configureServer(server){server.middlewares.use('/__vnet/read',async(req,res)=>{
 res.setHeader('Content-Type','application/json');try{
  if(req.method!=='POST'||!req.headers['content-type']?.startsWith('application/json')||!req.headers.origin||new URL(req.headers.origin).host!==req.headers.host)throw Error('拒绝非同源请求')
  let raw='';for await(const b of req){raw+=b;if(raw.length>32768)throw Error('请求过大')}
  const {purpose,args=[]}=JSON.parse(raw);if(!Array.isArray(args)||args.some(a=>typeof a!=='string'))throw Error('无效参数')
  const prl='/usr/local/bin/prlctl';let bin=path.resolve(import.meta.dirname,'../../VNet Studio.app/Contents/MacOS/vnetctl-cli');let argv:string[]=[]
  if(purpose==='vms'){bin=prl;argv=['list','-a','-i','-j']}
  else if(purpose==='host-read')argv=['host-network','list','--prlctl',prl]
  else if(purpose==='cli'){
   const a=[...args];if(a[0]==='--vm')a.splice(0,2)
   if(!['host-forward list','services list','routes list','vpn list','containers list','addresses','status'].includes(a.join(' ')))throw Error('浏览器预览仅支持读取；请在桌面应用执行操作')
   argv=[...args,'--prlctl',prl]
  }else throw Error('浏览器预览仅支持读取')
  const {stdout}=await exec(bin,argv,{timeout:30000,maxBuffer:16*1024*1024});let data=null;try{data=JSON.parse(stdout)}catch{}res.end(JSON.stringify({text:stdout,data}))
 }catch(e){res.statusCode=400;res.end(JSON.stringify({error:String(e)}))}
})}}
export default defineConfig({plugins:[vue(),tailwindcss(),readBridge],resolve:{alias:{'@':path.resolve(import.meta.dirname,'src')}},server:{host:'127.0.0.1',strictPort:true,watch:{ignored:['**/src-tauri/**','**/dist/**']}},build:{target:'safari15'}})
