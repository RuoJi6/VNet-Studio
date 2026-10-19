export const MIN_ZOOM = 0.4
export const MAX_ZOOM = 2
export interface Camera {zoom:number;x:number;y:number}
interface Point {x:number;y:number}
type PinchEvent = Event & {scale:number;clientX?:number;clientY?:number}

// Fold native scrolling into translation before zooming, keeping the same
// graph point beneath the pointer even when the scroll extent shrinks.
export function zoomAt(camera:Camera,requested:number,anchor:Point,scroll:Point):Camera {
 if(!Number.isFinite(requested)||requested<=0)return camera
 const zoom=Math.min(MAX_ZOOM,Math.max(MIN_ZOOM,requested))
 const ratio=zoom/camera.zoom
 return {zoom,x:anchor.x+(camera.x-scroll.x-anchor.x)*ratio,y:anchor.y+(camera.y-scroll.y-anchor.y)*ratio}
}

export function bindTopologyZoom(element:HTMLElement,getCamera:()=>Camera,update:(camera:Camera)=>void,onPinch:()=>void){
 let webkitPinch=false,lastScale=1
 function anchor(clientX?:number,clientY?:number):Point {
  const rect=element.getBoundingClientRect()
  return {
   x:Number.isFinite(clientX)?Math.max(0,Math.min(element.clientWidth,clientX!-rect.left-element.clientLeft)):element.clientWidth/2,
   y:Number.isFinite(clientY)?Math.max(0,Math.min(element.clientHeight,clientY!-rect.top-element.clientTop)):element.clientHeight/2,
  }
 }
 function setZoom(requested:number,clientX?:number,clientY?:number){
  const camera=getCamera()
  if(!Number.isFinite(requested)||requested<=0||Math.min(MAX_ZOOM,Math.max(MIN_ZOOM,requested))===camera.zoom)return
  const next=zoomAt(camera,requested,anchor(clientX,clientY),{x:element.scrollLeft,y:element.scrollTop})
  element.scrollLeft=0;element.scrollTop=0
  update(next)
 }
 function wheel(event:WheelEvent){
  // A trackpad pinch arrives as ctrl+wheel in Chromium. Ordinary two-finger
  // scrolling must keep its native behavior.
  if(!event.ctrlKey)return
  event.preventDefault();onPinch()
  if(webkitPinch)return // WebKit may also emit wheel events for the same pinch.
  const pixels=event.deltaY*(event.deltaMode===1?16:event.deltaMode===2?element.clientHeight:1)
  setZoom(getCamera().zoom*Math.exp(-Math.max(-100,Math.min(100,pixels))*.01),event.clientX,event.clientY)
 }
 function start(event:Event){
  event.preventDefault();onPinch();webkitPinch=true
  const scale=(event as PinchEvent).scale;lastScale=Number.isFinite(scale)&&scale>0?scale:1
 }
 function change(event:Event){
  event.preventDefault();onPinch()
  if(!webkitPinch)return
  const pinch=event as PinchEvent
  if(!Number.isFinite(pinch.scale)||pinch.scale<=0)return
  setZoom(getCamera().zoom*pinch.scale/lastScale,pinch.clientX,pinch.clientY)
  lastScale=pinch.scale
 }
 function end(event:Event){event.preventDefault();webkitPinch=false;lastScale=1;onPinch()}
 element.addEventListener('wheel',wheel,{passive:false})
 element.addEventListener('gesturestart',start,{passive:false})
 element.addEventListener('gesturechange',change,{passive:false})
 element.addEventListener('gestureend',end,{passive:false})
 return {setZoom,dispose(){
  element.removeEventListener('wheel',wheel)
  element.removeEventListener('gesturestart',start)
  element.removeEventListener('gesturechange',change)
  element.removeEventListener('gestureend',end)
 }}
}
