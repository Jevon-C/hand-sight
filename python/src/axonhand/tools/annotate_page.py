"""The page of the tap annotation tool, served by ``axonhand.tools.annotate``."""

PAGE = r"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Mark the taps</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
 body{font:15px system-ui,sans-serif;margin:0;background:#111;color:#eee}
 main{max-width:960px;margin:0 auto;padding:12px}
 video{width:100%;max-height:70vh;background:#000}
 .bar{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin:8px 0}
 button{font:inherit;padding:6px 12px;border-radius:6px;border:1px solid #666;background:#222;color:#eee;cursor:pointer}
 button.primary{background:#2b6cb0;border-color:#2b6cb0}
 input{font:inherit;padding:5px;background:#222;color:#eee;border:1px solid #666;border-radius:6px}
 #strip{position:relative;height:28px;background:#222;border-radius:4px;cursor:pointer}
 .mark{position:absolute;top:0;width:2px;height:28px;background:#f6ad55}
 #head{position:absolute;top:0;width:2px;height:28px;background:#63b3ed}
 kbd{background:#333;border-radius:4px;padding:1px 5px}
 #status{min-height:1.4em}
</style></head><body><main>
<h1 style="font-size:20px">Mark the taps: <span id="name"></span></h1>
<p>Mark each tap at the frame where the thumb and index fingertips <b>touch</b> as the fingers close (the first frame of contact).
Keys: <kbd>Space</kbd> play/pause, <kbd>,</kbd> and <kbd>.</kbd> one frame back/forward,
<kbd>Shift</kbd>+<kbd>,</kbd>/<kbd>.</kbd> ten frames, <kbd>M</kbd> mark here,
<kbd>Backspace</kbd> remove the last mark, <kbd>S</kbd> save, <kbd>[</kbd> <kbd>]</kbd> slower/faster.</p>
<video id="v" controls muted playsinline></video>
<div id="strip"><div id="head"></div></div>
<div class="bar">
 <button id="mark" class="primary">Mark tap (M)</button>
 <button id="undo">Remove last (Backspace)</button>
 <button id="save">Save (S)</button>
 <span id="count"></span><span id="time"></span>
</div>
<div class="bar">
 <label>Your name <input id="who" size="12"></label>
 <label>Note <input id="note" size="40" placeholder="lighting, tremor, hand out of view..."></label>
</div>
<div id="status"></div>
<script>
const v=document.getElementById('v'),strip=document.getElementById('strip'),head=document.getElementById('head');
let fps=30,marks=[],dirty=false;
const $=id=>document.getElementById(id);
const say=t=>{$('status').textContent=t};
function draw(){
 strip.querySelectorAll('.mark').forEach(n=>n.remove());
 const d=v.duration||1;
 for(const t of marks){const m=document.createElement('div');m.className='mark';m.style.left=(100*t/d)+'%';strip.appendChild(m)}
 head.style.left=(100*v.currentTime/d)+'%';
 $('count').textContent=marks.length+' taps marked'+(dirty?' (not saved)':'');
 $('time').textContent=' time '+v.currentTime.toFixed(3)+' s, frame '+Math.round(v.currentTime*fps);
}
function snap(t){return Math.round(t*fps)/fps}
function mark(){
 const t=snap(v.currentTime);
 if(marks.some(m=>Math.abs(m-t)<0.5/fps)){say('Already marked at this frame.');return}
 marks.push(t);marks.sort((a,b)=>a-b);dirty=true;draw();say('Marked '+t.toFixed(3)+' s');
}
function undo(){if(marks.length){marks.pop();dirty=true;draw()}}
function step(n){v.pause();v.currentTime=Math.min(Math.max(0,snap(v.currentTime)+n/fps+0.0001),v.duration)}
async function save(){
 const body={tap_times:marks,annotator:$('who').value,note:$('note').value};
 const r=await fetch('/save',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
 const j=await r.json();
 if(r.ok){dirty=false;draw();say('Saved '+marks.length+' taps to '+j.path)}else say('Not saved: '+j.error);
}
document.addEventListener('keydown',e=>{
 if(e.target.tagName==='INPUT')return;
 const k=e.key;
 if(k===' '){e.preventDefault();v.paused?v.play():v.pause()}
 else if(k===','||k==='<'){e.preventDefault();step(e.shiftKey?-10:-1)}
 else if(k==='.'||k==='>'){e.preventDefault();step(e.shiftKey?10:1)}
 else if(k==='m'||k==='M'){mark()}
 else if(k==='Backspace'){e.preventDefault();undo()}
 else if(k==='s'||k==='S'){save()}
 else if(k==='['){v.playbackRate=Math.max(0.1,v.playbackRate-0.1)}
 else if(k===']'){v.playbackRate=Math.min(2,v.playbackRate+0.1)}
});
strip.addEventListener('click',e=>{const r=strip.getBoundingClientRect();v.currentTime=(e.clientX-r.left)/r.width*v.duration});
$('mark').onclick=mark;$('undo').onclick=undo;$('save').onclick=save;
v.addEventListener('timeupdate',draw);v.addEventListener('seeked',draw);
window.addEventListener('beforeunload',e=>{if(dirty){e.preventDefault()}});
(async()=>{
 const info=await (await fetch('/info')).json();
 fps=info.fps;$('name').textContent=info.video;
 $('who').value=info.annotator||'';$('note').value=info.note||'';marks=info.tap_times;
 v.src='/video';v.onloadedmetadata=()=>{say('Loaded: '+info.fps.toFixed(2)+' frames per second.');draw()};
})();
</script></main></body></html>
"""
