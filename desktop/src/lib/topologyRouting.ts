import {NODE_WIDTH as W,NODE_HEIGHT as H,type TopologyNode} from './topologyGraph.ts'
interface Point {x:number;y:number}
interface Box {left:number;right:number;top:number;bottom:number}
const inside=(p:Point,b:Box)=>p.x>b.left&&p.x<b.right&&p.y>b.top&&p.y<b.bottom
function placeLabel(point:Point,width:number,nodes:TopologyNode[]){
 for(const offset of [-12,20]){
  const text={x:point.x,y:point.y+offset}
  if(!nodes.some(n=>text.x-width/2<n.x+W&&text.x+width/2>n.x&&text.y-11<n.y+H&&text.y+4>n.y))return text
 }
 return undefined
}
function crosses(a:Point,b:Point,r:Box){return a.x===b.x?a.x>r.left&&a.x<r.right&&Math.max(a.y,b.y)>r.top&&Math.min(a.y,b.y)<r.bottom:a.y>r.top&&a.y<r.bottom&&Math.max(a.x,b.x)>r.left&&Math.min(a.x,b.x)<r.right}
function ports(a:TopologyNode,b:TopologyNode):[Point,Point,Point,Point]{
 if(b.x>=a.x+W+32)return [{x:a.x+W,y:a.y+H/2},{x:a.x+W+20,y:a.y+H/2},{x:b.x-20,y:b.y+H/2},{x:b.x,y:b.y+H/2}]
 if(a.x>=b.x+W+32)return [{x:a.x,y:a.y+H/2},{x:a.x-20,y:a.y+H/2},{x:b.x+W+20,y:b.y+H/2},{x:b.x+W,y:b.y+H/2}]
 if(b.y>=a.y+H)return [{x:a.x+W/2,y:a.y+H},{x:a.x+W/2,y:a.y+H+20},{x:b.x+W/2,y:b.y-20},{x:b.x+W/2,y:b.y}]
 return [{x:a.x+W/2,y:a.y},{x:a.x+W/2,y:a.y-20},{x:b.x+W/2,y:b.y+H+20},{x:b.x+W/2,y:b.y+H}]
}
// Rectilinear visibility grid: preserve user positions, route around cards,
// and penalize bends. Camera movement does not rerun this calculation.
export function routeTopologyEdge(a:TopologyNode,b:TopologyNode,nodes:TopologyNode[],label:string){
 const [from,start,finish,to]=ports(a,b)
 const horizontal=from.y===start.y
 const reach=Math.min(72,Math.max(24,(horizontal?Math.abs(to.x-from.x):Math.abs(to.y-from.y))/2))
 const c1={x:from.x+(start.x-from.x)/20*reach,y:from.y+(start.y-from.y)/20*reach}
 const c2={x:to.x+(finish.x-to.x)/20*reach,y:to.y+(finish.y-to.y)/20*reach}
 const curve=(t:number)=>({x:(1-t)**3*from.x+3*(1-t)**2*t*c1.x+3*(1-t)*t*t*c2.x+t**3*to.x,y:(1-t)**3*from.y+3*(1-t)**2*t*c1.y+3*(1-t)*t*t*c2.y+t**3*to.y})
 const bounds={left:Math.min(from.x,c1.x,c2.x,to.x),right:Math.max(from.x,c1.x,c2.x,to.x),top:Math.min(from.y,c1.y,c2.y,to.y),bottom:Math.max(from.y,c1.y,c2.y,to.y)}
 const otherBoxes=nodes.filter(n=>n!==a&&n!==b).map(n=>({left:n.x-6,right:n.x+W+6,top:n.y-6,bottom:n.y+H+6})).filter(r=>r.left<=bounds.right&&r.right>=bounds.left&&r.top<=bounds.bottom&&r.bottom>=bounds.top)
 const samples=Array.from({length:65},(_,i)=>curve(i/64))
 if(samples.every(p=>!otherBoxes.some(r=>inside(p,r)))){
  const mid=curve(.5),width=[...label].reduce((sum,c)=>sum+(/[^\x00-\xff]/.test(c)?11:6),6)
  const text=horizontal?placeLabel(mid,width,nodes):undefined
  return {points:samples,path:`M${from.x},${from.y} C${c1.x},${c1.y} ${c2.x},${c2.y} ${to.x},${to.y}`,text}
 }
 const boxes=nodes.map(n=>({left:n.x-12,right:n.x+W+12,top:n.y-12,bottom:n.y+H+12}))
 const middle=horizontal?(start.x+finish.x)/2:(start.y+finish.y)/2
 const direct=horizontal?[start,{x:middle,y:start.y},{x:middle,y:finish.y},finish]:[start,{x:start.x,y:middle},{x:finish.x,y:middle},finish]
 if(direct.every((p,i)=>i===0||!boxes.some(r=>inside(p,r)||crosses(direct[i-1]!,p,r))))return finishRoute([from,...direct,to],label,nodes)
 const xs=[...new Set([start.x,finish.x,...boxes.flatMap(r=>[r.left,r.right])])].sort((a,b)=>a-b)
 const ys=[...new Set([start.y,finish.y,...boxes.flatMap(r=>[r.top,r.bottom])])].sort((a,b)=>a-b)
 const ny=ys.length,point=(id:number)=>({x:xs[Math.floor(id/ny)]!,y:ys[id%ny]!})
 const startId=xs.indexOf(start.x)*ny+ys.indexOf(start.y),endId=xs.indexOf(finish.x)*ny+ys.indexOf(finish.y)
 const distance=new Map<number,number>(),previous=new Map<number,number>(),heap:{key:number;cost:number;priority:number}[]=[]
 function push(value:typeof heap[number]){heap.push(value);let i=heap.length-1;while(i>0){const parent=(i-1)>>1;if(heap[parent]!.priority<=value.priority)break;heap[i]=heap[parent]!;i=parent}heap[i]=value}
 function pop(){const first=heap[0]!,last=heap.pop()!;if(heap.length){let i=0;while(i*2+1<heap.length){let child=i*2+1;if(child+1<heap.length&&heap[child+1]!.priority<heap[child]!.priority)child++;if(heap[child]!.priority>=last.priority)break;heap[i]=heap[child]!;i=child}heap[i]=last}return first}
 const startKey=startId*3+(from.y===start.y?0:1)
 distance.set(startKey,0);push({key:startKey,cost:0,priority:0})
 let endKey:number|undefined
 while(heap.length){
  const current=pop();if(current.cost!==distance.get(current.key))continue
  const id=Math.floor(current.key/3),direction=current.key%3,p=point(id)
  if(id===endId){endKey=current.key;break}
  const xi=Math.floor(id/ny),yi=id%ny
  for(const [nx,nyIndex,d] of [[xi-1,yi,0],[xi+1,yi,0],[xi,yi-1,1],[xi,yi+1,1]]){
   if(nx!<0||nx!>=xs.length||nyIndex!<0||nyIndex!>=ys.length)continue
   const nextId=nx!*ny+nyIndex!,q=point(nextId)
   if(boxes.some(r=>inside(q,r)||crosses(p,q,r)))continue
   const key=nextId*3+d!,cost=current.cost+Math.abs(q.x-p.x)+Math.abs(q.y-p.y)+(direction===d?0:24)
   if(cost>=(distance.get(key)??Infinity))continue
   distance.set(key,cost);previous.set(key,current.key)
   push({key,cost,priority:cost+Math.abs(q.x-finish.x)+Math.abs(q.y-finish.y)})
  }
 }
 let points:Point[]=[]
 if(endKey!==undefined){let key:number|undefined=endKey;while(key!==undefined){points.unshift(point(Math.floor(key/3)));key=previous.get(key)}}
 else points=[start,{x:start.x,y:Math.max(...boxes.map(b=>b.bottom))+20},{x:finish.x,y:Math.max(...boxes.map(b=>b.bottom))+20},finish]
 return finishRoute([from,...points,to],label,nodes)
}
function finishRoute(points:Point[],label:string,nodes:TopologyNode[]){
 points=points.filter((p,i,all)=>i===0||p.x!==all[i-1]!.x||p.y!==all[i-1]!.y)
 points=points.filter((p,i,all)=>i===0||i===all.length-1||!((all[i-1]!.x===p.x&&p.x===all[i+1]!.x)||(all[i-1]!.y===p.y&&p.y===all[i+1]!.y)))
 const width=[...label].reduce((sum,c)=>sum+(/[^\x00-\xff]/.test(c)?11:6),6)
 let text:Point|undefined
 for(let i=points.length-2;i>=0;i--){const p=points[i]!,q=points[i+1]!;if(p.y!==q.y||Math.abs(p.x-q.x)<width)continue
  text=placeLabel({x:(p.x+q.x)/2,y:p.y},width,nodes)
  if(text)break
 }
 let path=`M${points[0]!.x},${points[0]!.y}`
 for(let i=1;i<points.length;i++){
  const previous=points[i-1]!,p=points[i]!,next=points[i+1]
  if(!next){path+=` L${p.x},${p.y}`;continue}
  const radius=Math.min(18,Math.hypot(p.x-previous.x,p.y-previous.y)/2,Math.hypot(next.x-p.x,next.y-p.y)/2)
  const before={x:p.x+Math.sign(previous.x-p.x)*radius,y:p.y+Math.sign(previous.y-p.y)*radius}
  const after={x:p.x+Math.sign(next.x-p.x)*radius,y:p.y+Math.sign(next.y-p.y)*radius}
  path+=` L${before.x},${before.y} Q${p.x},${p.y} ${after.x},${after.y}`
 }
 return {points,path,text}
}
