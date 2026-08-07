use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use obscura_browser::{BrowserContext, Page, WaitUntil};
use obscura_net::ResourceType;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let run_id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let dump_dir = std::path::PathBuf::from("target/turnstile-dumps").join(run_id.to_string());
    std::fs::create_dir_all(&dump_dir).unwrap();
    let manifest = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let next_file = Arc::new(AtomicUsize::new(1));

    let context = Arc::new(BrowserContext::with_full_options(
        "managed-turnstile-probe".to_string(), None, true, None,
    ));
    let mut page = Page::new("managed-turnstile-probe-page".to_string(), context);
    let request_manifest = manifest.clone();
    page.on_request(Arc::new(move |request| {
        request_manifest.lock().unwrap().push(serde_json::json!({
            "kind": "request", "url": request.url, "method": request.method,
            "resourceType": format!("{:?}", request.resource_type),
        }));
    }));
    let response_dir = dump_dir.clone();
    let response_manifest = manifest.clone();
    let response_next = next_file.clone();
    page.on_response(Arc::new(move |request, response| {
        response_manifest.lock().unwrap().push(serde_json::json!({
            "kind": "response", "url": response.url, "status": response.status,
            "resourceType": format!("{:?}", request.resource_type),
            "contentType": response.content_type(), "bytes": response.body.len(),
        }));
        let is_javascript = request.resource_type == ResourceType::Script
            || request.resource_type == ResourceType::Fetch
            || response.content_type().is_some_and(|value| value.contains("javascript"));
        if !is_javascript { return; }
        let number = response_next.fetch_add(1, Ordering::Relaxed);
        let name = format!("{number:04}-network.js");
        if std::fs::write(response_dir.join(&name), &response.body).is_ok() {
            response_manifest.lock().unwrap().push(serde_json::json!({
                "file": name, "kind": "network", "url": response.url,
                "contentType": response.content_type(), "bytes": response.body.len(),
            }));
        }
    }));
    if std::env::var_os("OBSCURA_PROBE_PST_STUB").is_some() {
        page.add_preload_script(
            "Document.prototype.hasPrivateToken=async function(){return false}; \
             Document.prototype.hasRedemptionRecord=async function(){return false};",
        );
    }
    if std::env::var_os("OBSCURA_PROBE_CHROME_SURFACES").is_some() {
        page.add_preload_script(
            "Document.prototype.hasPrivateToken=async function(){return false}; \
             Document.prototype.hasRedemptionRecord=async function(){return false}; \
             Object.defineProperty(Navigator.prototype,'maxTouchPoints',{get:function(){return 0},enumerable:true,configurable:true}); \
             globalThis.crossOriginIsolated=false; \
             if(typeof globalThis.TouchEvent==='undefined'){ \
               globalThis.TouchEvent=class TouchEvent extends UIEvent{}; \
             }",
        );
    }
    let large_eval_delay_ms = std::env::var("OBSCURA_PROBE_LARGE_EVAL_DELAY_MS")
        .ok().and_then(|value| value.parse::<u64>().ok()).unwrap_or(0).min(1_000);
    page.add_preload_script(&format!(
        "globalThis.__probeLargeEvalDelayMs={large_eval_delay_ms};"
    ));
    page.add_preload_script(
        "globalThis.__roots=[]; globalThis.__messages=[]; globalThis.__messageData=[]; globalThis.__incomingMessages=[]; globalThis.__capturedBlobs=[]; globalThis.__capturedDynamicJs=[]; \
         globalThis.__windowListeners=[]; globalThis.__documentListeners=[]; globalThis.__listenerSources=[]; globalThis.__dispatchCalls=[]; globalThis.__consoleErrors=[]; globalThis.__capturedWorkers=[]; globalThis.__capturedImages=[]; globalThis.__capturedLinks=[]; globalThis.__runProgramCalls=[]; globalThis.__probeXhrErrors=[]; globalThis.__probeWorkerScopeMessages=[]; globalThis.__probeApiCalls=[]; \
         const nativeEval=globalThis.eval; \
         globalThis.__nativeEvalShape={text:String(nativeEval),nativeText:Function.prototype.toString.call(nativeEval),name:nativeEval.name,length:nativeEval.length,keys:Reflect.ownKeys(nativeEval),proto:Object.getPrototypeOf(nativeEval)===Function.prototype,completion:Reflect.apply(nativeEval,globalThis,['0,/.*honk.*/,123456789'])}; \
         try{Reflect.apply(nativeEval,globalThis,[\"throw new Error('eval-probe')\"])}catch(e){globalThis.__nativeEvalErrorStack=String(e.stack)} \
         globalThis.eval=new Proxy(nativeEval,{ \
           apply(target,thisArg,args){ \
             let result; const record={kind:'eval',text:String(args[0]||''),started:Date.now()}; \
             if(record.text.length>1000000&&!globalThis.__largeEvalStack){ \
               globalThis.__largeEvalStack=record.stack=String(new Error().stack||''); \
             } \
             try { result=Reflect.apply(target,thisArg,args); return result; } \
             finally { \
               if(record.text.length>1000000&&globalThis.__probeLargeEvalDelayMs>0){ \
                 const until=performance.now()+globalThis.__probeLargeEvalDelayMs; \
                 while(performance.now()<until){} \
               } \
               record.ended=Date.now(); record.resultType=typeof result; \
               if(result==null||typeof result==='string'||typeof result==='number'||typeof result==='boolean')record.result=String(result); \
               try{__capturedDynamicJs.push(record)}catch(_){} \
             } \
           }, \
           get(target,name,receiver){if(name==='toString')return target.toString.bind(target);return Reflect.get(target,name,receiver)} \
         }); \
         globalThis.eval=nativeEval; \
         const oldConsoleError=console.error; \
         console.error=function(...args){ \
           __consoleErrors.push(args.map(value=>{try{return value instanceof Error?{name:value.name,message:value.message,stack:String(value.stack||'')}:{value:String(value)}}catch(error){return {captureError:String(error)}}})); \
           return oldConsoleError.apply(this,args); \
         }; \
         const oldDispatch=globalThis.__obscura_dispatchMouse; \
         globalThis.__obscura_dispatchMouse=function(type,x,y,count){ \
           try { const target=document.elementFromPoint(x,y); __dispatchCalls.push({type,x,y,target:target&&target.tagName}); return oldDispatch(type,x,y,count); } \
           catch(error) { __dispatchCalls.push({type,error:String(error)}); throw error; } \
         }; \
         const oldWindowAdd=globalThis.addEventListener; \
         globalThis.addEventListener=function(type,handler,opts){__windowListeners.push(String(type));__listenerSources.push({target:'window',type:String(type),source:String(handler)});return oldWindowAdd.call(this,type,handler,opts)}; \
         const oldDocumentAdd=document.addEventListener; \
         document.addEventListener=function(type,handler,opts){__documentListeners.push(String(type));__listenerSources.push({target:'document',type:String(type),source:String(handler)});return oldDocumentAdd.call(this,type,handler,opts)}; \
         const oldAttach=Element.prototype.attachShadow; \
         Element.prototype.attachShadow=function(init){var r=oldAttach.call(this,init);__roots.push(r);return r}; \
         const wrapRunProgram=function(){ \
           if(globalThis.__runProgramWrapped||typeof globalThis.runProgram!=='function')return; \
           const nativeRunProgram=globalThis.runProgram; \
           globalThis.runProgram=new Proxy(nativeRunProgram,{apply(target,thisArg,args){ \
             const record={started:Date.now(),inputType:typeof args[0],inputLength:typeof args[0]==='string'?args[0].length:null}; \
             __runProgramCalls.push(record); \
             try { const result=Reflect.apply(target,thisArg,args); record.ended=Date.now();record.resultType=typeof result;record.resultText=typeof result==='function'?String(result).slice(0,500):String(result); \
               if(typeof result!=='function')return result; \
               record.callbackInvocations=[]; \
               return new Proxy(result,{apply(callback,callbackThis,callbackArgs){ \
                 const invocation={started:Date.now(),argTypes:callbackArgs.map(value=>typeof value)}; record.callbackInvocations.push(invocation); \
                 try { const callbackResult=Reflect.apply(callback,callbackThis,callbackArgs); invocation.ended=Date.now();invocation.resultType=typeof callbackResult;return callbackResult; } \
                 catch(error){invocation.ended=Date.now();invocation.error=String(error);invocation.stack=String(error&&error.stack||'');throw error} \
               }}); \
             } \
             catch(error){record.ended=Date.now();record.error=String(error);record.stack=String(error&&error.stack||'');throw error} \
           }}); \
           globalThis.__runProgramWrapped=true; \
         }; \
         addEventListener('message',e=>{ \
           const data=e.data; __messages.push(data&&data.event||'?'); \
           if(data&&data.event==='execute')wrapRunProgram(); \
           try { __messageData.push(JSON.parse(JSON.stringify(data))); } catch(_) {} \
           try { __incomingMessages.push({event:data&&data.event||'?',keys:data&&typeof data==='object'?Object.keys(data):[],types:data&&typeof data==='object'?Object.fromEntries(Object.entries(data).map(([key,value])=>[key,typeof value])):{}}); } catch(_) {} \
         }); \
         const oldElementAdd=Element.prototype.addEventListener; \
         Element.prototype.addEventListener=function(type,handler,opts){ \
           (this.__capturedListeners||(this.__capturedListeners=[])).push(String(type)); __listenerSources.push({target:this.tagName||'element',type:String(type),source:String(handler)}); \
           return oldElementAdd.call(this,type,handler,opts); \
         }; \
         const recordImage=function(img,value,via){ \
           const record={value:String(value),via,assigned:Date.now(),stack:String(new Error('image-src').stack||'')}; \
           __capturedImages.push(record); \
           try { oldElementAdd.call(img,'load',()=>{record.loadAt=Date.now();record.complete=img.complete;record.naturalWidth=img.naturalWidth;record.naturalHeight=img.naturalHeight}); } catch(_) {} \
           try { oldElementAdd.call(img,'error',e=>{record.errorAt=Date.now();record.error=String(e&&e.message||e)}); } catch(_) {} \
         }; \
         const NativeImage=globalThis.Image; \
         const wrapImage=function(img){ \
           try { const desc=Object.getOwnPropertyDescriptor(img,'src'); if(desc&&desc.configurable&&desc.set){ \
             Object.defineProperty(img,'src',{configurable:desc.configurable,enumerable:desc.enumerable,get:desc.get,set(value){recordImage(img,value,'Image.src');return desc.set.call(img,value)}}); \
           }} catch(_) {} return img; \
         }; \
         globalThis.Image=new Proxy(NativeImage,{construct(target,args,newTarget){return wrapImage(Reflect.construct(target,args,newTarget))},apply(target,thisArg,args){return wrapImage(Reflect.apply(target,thisArg,args))}}); \
         const oldSetAttribute=Element.prototype.setAttribute; \
         Element.prototype.setAttribute=function(name,value){ \
           if(this.tagName==='IMG'&&String(name).toLowerCase()==='src')recordImage(this,value,'setAttribute'); \
           if(this.tagName==='LINK')__capturedLinks.push({stage:'setAttribute',name:String(name),value:String(value),at:Date.now(),stack:String(new Error('link-attribute').stack||'')}); \
           return oldSetAttribute.call(this,name,value); \
         }; \
         const recordLinkInsert=function(link,via){if(link&&link.tagName==='LINK')__capturedLinks.push({stage:'insert',via,at:Date.now(),rel:link.rel||link.getAttribute('rel')||'',as:link.as||link.getAttribute('as')||'',href:link.href||link.getAttribute('href')||'',outerHTML:link.outerHTML||'',stack:String(new Error('link-insert').stack||'')})}; \
         const oldAppendChild=Node.prototype.appendChild; \
         Node.prototype.appendChild=function(child){recordLinkInsert(child,'appendChild');return oldAppendChild.call(this,child)}; \
         const oldInsertBefore=Node.prototype.insertBefore; \
         Node.prototype.insertBefore=function(child,before){recordLinkInsert(child,'insertBefore');return oldInsertBefore.call(this,child,before)}; \
         const oldObjectURL=URL.createObjectURL; \
         URL.createObjectURL=function(blob){ \
           const url=oldObjectURL.call(URL,blob); \
           try { \
             const record={url,type:blob&&blob.type||'',created:Date.now(),stack:String(new Error('blob-created').stack||'')}; \
             if(blob&&blob._bytes) { record.text=new TextDecoder().decode(blob._bytes); __capturedBlobs.push(record); } \
             else if(blob&&blob.text) blob.text().then(text=>{record.text=text;__capturedBlobs.push(record)}); \
           } catch(_) {} \
           return url; \
         }; \
         const NativeWorker=globalThis.Worker; \
         globalThis.Worker=new Proxy(NativeWorker,{construct(target,args){ \
           const sourceUrl=String(args[0]||''); \
           const record={url:sourceUrl,options:null,blobSource:globalThis.__blobStore&&globalThis.__blobStore[sourceUrl]||'',created:Date.now(),constructStack:String(new Error('worker-created').stack||''),posts:[],messages:[],scopeMessages:[],errors:[],terminated:false}; \
           try { record.options=JSON.parse(JSON.stringify(args[1]||null)); } catch(_) {} \
           __capturedWorkers.push(record); \
           let worker; \
           try { worker=Reflect.construct(target,args,target); } \
           catch(error) { record.errors.push({stage:'construct',message:String(error),stack:String(error&&error.stack||'')}); throw error; } \
           const nativePost=worker.postMessage; \
           worker.postMessage=function(data,...rest){ \
             try { record.posts.push(JSON.parse(JSON.stringify(data))); } catch(_) { record.posts.push(String(data)); } \
             try { const scope=worker._scope; const handler=scope&&scope.onmessage; if(handler&&!handler.__probeWrapped){ \
               const wrapped=new Proxy(handler,{apply(target,thisArg,args){const event=args[0];const state={isTrusted:event&&event.isTrusted,origin:event&&event.origin,source:event&&event.source,type:event&&event.type,started:Date.now()};record.scopeMessages.push(state);try{const result=Reflect.apply(target,thisArg,args);state.ended=Date.now();state.resultType=typeof result;return result}catch(error){state.ended=Date.now();state.error=String(error);state.stack=String(error&&error.stack||'');throw error}}}); \
               Object.defineProperty(wrapped,'__probeWrapped',{value:true});scope.onmessage=wrapped; \
             }} catch(error) { record.errors.push({stage:'scope-wrap',message:String(error),stack:String(error&&error.stack||'')}); } \
             return nativePost.call(this,data,...rest); \
           }; \
           const nativeTerminate=worker.terminate; \
           worker.terminate=function(){ record.terminated=true; record.terminatedAt=Date.now(); record.terminateStack=String(new Error('worker-terminated').stack||''); return nativeTerminate.call(this); }; \
           worker.addEventListener('message',event=>{try{record.messages.push(JSON.parse(JSON.stringify(event.data)))}catch(_){record.messages.push(String(event.data))}}); \
           worker.addEventListener('error',event=>record.errors.push({stage:'run',message:String(event&&event.message||event),stack:String(event&&event.error&&event.error.stack||'')})); \
           setTimeout(()=>{try{record.resolvedUrl=worker._url||sourceUrl;record.code=worker._code||record.blobSource||''}catch(_){}},0); \
           return worker; \
         }});",
    );
    let navigation = page
        .navigate_with_wait("https://turnstile-test.vercel.app/", WaitUntil::DomContentLoaded)
        .await;
    println!("navigation: {navigation:?}");
    if navigation.is_err() {
        println!("session dump: {}", dump_dir.display());
        return;
    }
    for _ in 0..5 { page.settle(1_000).await; }

    println!("frames: {:?}", page.frame_urls());
    if page.frame_urls().is_empty() {
        dump_realm_sources(&mut page, None, &dump_dir, &manifest, &next_file);
        std::fs::write(
            dump_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&*manifest.lock().unwrap()).unwrap(),
        ).unwrap();
        println!("session dump: {}", dump_dir.display());
        return;
    }
    println!("frame state: {}", page.evaluate_in_frame(0, r#"JSON.stringify((() => {
      const root=(globalThis.__roots||[])[0];
      const items=root&&root.querySelectorAll?Array.from(root.querySelectorAll('*')).map(e=>({
        tag:e.tagName,id:e.id,cls:String(e.className||''),type:e.getAttribute&&e.getAttribute('type'),
        role:e.getAttribute&&e.getAttribute('role'),tabindex:e.getAttribute&&e.getAttribute('tabindex'),
        aria:e.getAttribute&&e.getAttribute('aria-label'),style:e.style&&e.style.cssText,
        listeners:e.__capturedListeners||[]
      })):[];
      const describe=e=>e&&({tag:e.tagName,id:e.id,cls:e.className,role:e.getAttribute&&e.getAttribute('role'),
        type:e.getAttribute&&e.getAttribute('type'),listeners:e._listeners&&Object.keys(e._listeners)});
      return {at10:describe(document.elementFromPoint(50,10)),
        at30:describe(document.elementFromPoint(30,30)),
        errors:globalThis.__obscura_errors||[], typeofOz:typeof globalThis.Oz,
        privateTokenApis:[typeof document.hasPrivateToken,typeof document.hasRedemptionRecord],
        windowListeners:globalThis.__windowListeners,documentListeners:globalThis.__documentListeners,
        shadow:!!root, itemCount:items.length, clickListeners:items.filter(item=>item.listeners.includes('click'))};
    })())"#));
    println!("frame bubble: {}", page.evaluate_in_frame(0, r#"(()=>{let seen=0,mouse='ok',pointer='ok';
      document.addEventListener('obscura-probe',()=>seen++);
      document.body.dispatchEvent(new Event('obscura-probe',{bubbles:true}));
      try{document.body.dispatchEvent(__obscura_markTrusted(new MouseEvent('obscura-probe',{bubbles:true})))}catch(e){mouse=String(e)}
      try{document.body.dispatchEvent(__obscura_markTrusted(new PointerEvent('obscura-probe',{bubbles:true})))}catch(e){pointer=String(e)}
      return {seen,same:document.documentElement.parentNode===document,
        mouse,pointer,mouseType:typeof MouseEvent,pointerType:typeof PointerEvent,
        bodyParent:document.body.parentNode&&document.body.parentNode.tagName,
        htmlParentType:document.documentElement.parentNode&&document.documentElement.parentNode.nodeType};})()"#));
    page.evaluate_in_frame(0, r#"(()=>{globalThis.__probeEvents=[];
      for(const type of ['pointerdown','mousedown','pointerup','mouseup','click'])
        document.addEventListener(type,e=>__probeEvents.push({type,trusted:e.isTrusted,
          target:e.target&&e.target.tagName,clientX:e.clientX,clientY:e.clientY,screenX:e.screenX,screenY:e.screenY,
          pageX:e.pageX,pageY:e.pageY,offsetX:e.offsetX,offsetY:e.offsetY,movementX:e.movementX,movementY:e.movementY,
          button:e.button,buttons:e.buttons,detail:e.detail,composed:e.composed,viewIsWindow:e.view===window,bubbles:e.bubbles}));return true})()"#);

    let rect = page.evaluate("(function(){var f=__roots[0].querySelector('iframe'),r=f.getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height}})()");
    let x = rect["x"].as_f64().unwrap() + rect["w"].as_f64().unwrap() / 2.0;
    let y = rect["y"].as_f64().unwrap() + rect["h"].as_f64().unwrap() / 2.0;
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        println!("{kind} routed={}", page.dispatch_mouse_event(kind, x, y, 1));
    }
    let rounds = std::env::var("OBSCURA_PROBE_ROUNDS")
        .ok().and_then(|value| value.parse::<usize>().ok()).unwrap_or(60);
    for round in 1..=rounds {
        page.settle(1_000).await;
        if round % 5 == 0 {
            let click = page.evaluate_in_frame(0, r#"(()=>{
              const roots=globalThis.__roots||[];
              const input=roots.map(root=>root.querySelector&&root.querySelector('input[type=checkbox]')).find(Boolean);
              if(!input)return {found:false};
              const r=input.getBoundingClientRect(),x=r.left+r.width/2,y=r.top+r.height/2;
              for(const type of ['mouseMoved','mousePressed','mouseReleased'])
                globalThis.__obscura_dispatchMouse(type,x,y,1);
              return {found:true,x,y,target:(document.elementFromPoint(x,y)||{}).tagName,checked:input.checked};
            })()"#);
            println!("round {round} shadow click: {click}");
        }
        let page_state = decode_json_string(page.evaluate(r#"JSON.stringify({token:(document.querySelector('input[name="cf-turnstile-response"]')||{}).value||'',messages:__messages,messageData:(__messageData||[]).slice(-100)})"#));
        let frame_state = decode_json_string(page.evaluate_in_frame(0, r#"JSON.stringify((()=>{const d=e=>e&&({tag:e.tagName,id:e.id,cls:e.className,role:e.getAttribute&&e.getAttribute('role'),type:e.getAttribute&&e.getAttribute('type')});const safe=fn=>{try{return fn()}catch(e){return 'error:'+e.name}};return {errors:globalThis.__obscura_errors||[],consoleErrors:globalThis.__consoleErrors||[],apiCalls:globalThis.__probeApiCalls||[],largeEvalStack:globalThis.__largeEvalStack||'',evalCalls:(globalThis.__capturedDynamicJs||[]).map(call=>({length:call.text.length,tail:call.text.slice(-80),started:call.started,ended:call.ended,duration:call.ended-call.started,resultType:call.resultType,result:call.result})).slice(-100),iframeEvalTrace:globalThis.__iframeEvalTrace||[],iframeWindowTrace:globalThis.__iframeWindowTrace||[],workers:(globalThis.__capturedWorkers||[]).map(worker=>({url:worker.url,resolvedUrl:worker.resolvedUrl,codeBytes:(worker.code||worker.blobSource||'').length,posts:worker.posts,messages:worker.messages,errors:worker.errors,terminated:worker.terminated})),vmTrace:globalThis.__vmTrace||null,vmMissing:globalThis.__vmMissing||[],vmGetsAll:(globalThis.__vmGets||[]).slice(-1000),vmReadsAll:(globalThis.__vmReads||[]).slice(-1000),vmGets:(globalThis.__vmGets||[]).filter(x=>x.valueType==='undefined').slice(-200),vmReads:(globalThis.__vmReads||[]).filter(x=>x.valueType==='undefined').slice(-200),vmUndefined:(globalThis.__vmUndefined||[]).slice(-100),messages:globalThis.__incomingMessages||[],events:globalThis.__probeEvents||[],calls:globalThis.__dispatchCalls||[],surface:{ua:navigator.userAgent,webdriver:navigator.webdriver,platform:navigator.platform,vendor:navigator.vendor,languages:Array.from(navigator.languages||[]),plugins:Array.from(navigator.plugins||[]).map(p=>[p.name,p.filename,p.length]),hardwareConcurrency:navigator.hardwareConcurrency,deviceMemory:navigator.deviceMemory,maxTouchPoints:navigator.maxTouchPoints,cookieEnabled:navigator.cookieEnabled,screen:[screen.width,screen.height,screen.availWidth,screen.availHeight,screen.colorDepth,screen.pixelDepth],viewport:[innerWidth,innerHeight,outerWidth,outerHeight,devicePixelRatio],timezone:safe(()=>Intl.DateTimeFormat().resolvedOptions().timeZone),chrome:safe(()=>Object.keys(chrome||{})),privateToken:[typeof document.hasPrivateToken,typeof document.hasRedemptionRecord],apis:['Worker','SharedWorker','WebAssembly','OffscreenCanvas','WebGLRenderingContext','PointerEvent','TouchEvent','PerformanceObserver','speechSynthesis','Notification'].map(k=>[k,typeof globalThis[k]]),cryptoRandomUUID:typeof crypto?.randomUUID,permissions:typeof navigator.permissions,mediaDevices:typeof navigator.mediaDevices,crossOriginIsolated:globalThis.crossOriginIsolated,evalShape:globalThis.__nativeEvalShape,dateNowShape:safe(()=>({text:String(Date.now),nativeText:Function.prototype.toString.call(Date.now),name:Date.now.name,length:Date.now.length,integer:Number.isInteger(Date.now())})),evalErrorStack:globalThis.__nativeEvalErrorStack,webdriverDescriptor:safe(()=>{const x=Object.getOwnPropertyDescriptor(Navigator.prototype,'webdriver');return x&&{enumerable:x.enumerable,configurable:x.configurable,get:String(x.get)}})},at10:d(document.elementFromPoint(50,10)),at30:d(document.elementFromPoint(30,30))}})())"#));
        let state = serde_json::json!({"page": page_state, "frame": frame_state});
        std::fs::write(
            dump_dir.join(format!("state-{round}.json")),
            serde_json::to_vec_pretty(&state).unwrap(),
        ).unwrap();
        let token_len = state["page"]["token"].as_str().map_or(0, str::len);
        let missing = state["frame"]["vmMissing"].as_array()
            .and_then(|items| items.last()).cloned().unwrap_or(serde_json::Value::Null);
        let undefined = state["frame"]["vmUndefined"].as_array()
            .and_then(|items| items.last()).cloned().unwrap_or(serde_json::Value::Null);
        println!("round {round}: token_len={token_len} vmMissing={missing} vmUndefined={undefined}");
    }

    dump_realm_sources(&mut page, None, &dump_dir, &manifest, &next_file);
    for index in 0..page.frame_urls().len() {
        dump_realm_sources(&mut page, Some(index), &dump_dir, &manifest, &next_file);
    }
    std::fs::write(
        dump_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&*manifest.lock().unwrap()).unwrap(),
    ).unwrap();
    println!("session dump: {}", dump_dir.display());
}

fn decode_json_string(value: serde_json::Value) -> serde_json::Value {
    value.as_str()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(value)
}

fn dump_realm_sources(
    page: &mut Page,
    frame: Option<usize>,
    dump_dir: &std::path::Path,
    manifest: &Mutex<Vec<serde_json::Value>>,
    next_file: &AtomicUsize,
) {
    let expression = r#"JSON.stringify({
      html:document.documentElement&&document.documentElement.outerHTML||'',
      scripts:Array.from(document.querySelectorAll('script')).map(s=>({src:s.src||'',text:s.textContent||''})),
      blobs:globalThis.__capturedBlobs||[],
      dynamics:globalThis.__capturedDynamicJs||[],
      shadows:(globalThis.__roots||[]).map(root=>root.innerHTML||''),
      listeners:globalThis.__listenerSources||[],
      workers:globalThis.__capturedWorkers||[],
      images:globalThis.__capturedImages||[],
      links:globalThis.__capturedLinks||[],
      runProgramCalls:globalThis.__runProgramCalls||[],
      xhrErrors:globalThis.__probeXhrErrors||[],
      workerScopeMessages:globalThis.__probeWorkerScopeMessages||[],
      runProgramType:typeof globalThis.runProgram,
      runProgramWrapped:!!globalThis.__runProgramWrapped
    })"#;
    let raw = match frame {
        Some(index) => page.evaluate_in_frame(index, expression),
        None => page.evaluate(expression),
    };
    let Some(raw) = raw.as_str() else { return };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else { return };
    let realm = frame.map_or_else(|| "page".to_string(), |index| format!("frame-{index}"));
    let html_name = format!("{realm}.html");
    let html = value["html"].as_str().unwrap_or_default();
    let _ = std::fs::write(dump_dir.join(&html_name), html);
    manifest.lock().unwrap().push(serde_json::json!({
        "file": html_name, "kind": "document", "realm": realm, "bytes": html.len(),
        "runProgramType": value["runProgramType"], "runProgramWrapped": value["runProgramWrapped"],
    }));
    for script in value["scripts"].as_array().into_iter().flatten() {
        let text = script["text"].as_str().unwrap_or_default();
        if text.is_empty() { continue }
        write_source(dump_dir, manifest, next_file, "inline", &realm,
            script["src"].as_str().unwrap_or_default(), text);
    }
    for blob in value["blobs"].as_array().into_iter().flatten() {
        write_source(dump_dir, manifest, next_file, "blob", &realm,
            blob["url"].as_str().unwrap_or_default(), blob["text"].as_str().unwrap_or_default());
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "blob-state", "realm": realm, "url": blob["url"],
            "created": blob["created"], "stack": blob["stack"],
        }));
    }
    for dynamic in value["dynamics"].as_array().into_iter().flatten() {
        write_source(dump_dir, manifest, next_file, "dynamic", &realm,
            dynamic["kind"].as_str().unwrap_or_default(), dynamic["text"].as_str().unwrap_or_default());
    }
    for worker in value["workers"].as_array().into_iter().flatten() {
        let source = worker["code"].as_str()
            .or_else(|| worker["blobSource"].as_str())
            .unwrap_or_default();
        let url = worker["resolvedUrl"].as_str()
            .or_else(|| worker["url"].as_str())
            .unwrap_or_default();
        write_source(dump_dir, manifest, next_file, "worker", &realm, url, source);
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "worker-state", "realm": realm, "url": url,
            "posts": worker["posts"], "messages": worker["messages"],
            "scopeMessages": worker["scopeMessages"],
            "errors": worker["errors"], "terminated": worker["terminated"],
            "created": worker["created"], "constructStack": worker["constructStack"],
            "terminatedAt": worker["terminatedAt"], "terminateStack": worker["terminateStack"],
        }));
    }
    for image in value["images"].as_array().into_iter().flatten() {
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "image-state", "realm": realm, "value": image["value"],
            "via": image["via"], "assigned": image["assigned"], "loadAt": image["loadAt"],
            "errorAt": image["errorAt"], "complete": image["complete"],
            "naturalWidth": image["naturalWidth"], "naturalHeight": image["naturalHeight"],
            "stack": image["stack"], "error": image["error"],
        }));
    }
    for link in value["links"].as_array().into_iter().flatten() {
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "link-state", "realm": realm, "stage": link["stage"],
            "via": link["via"], "at": link["at"], "name": link["name"],
            "value": link["value"], "rel": link["rel"], "as": link["as"],
            "href": link["href"], "outerHTML": link["outerHTML"], "stack": link["stack"],
        }));
    }
    for call in value["runProgramCalls"].as_array().into_iter().flatten() {
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "run-program", "realm": realm, "started": call["started"],
            "ended": call["ended"], "inputType": call["inputType"],
            "inputLength": call["inputLength"], "resultType": call["resultType"],
            "resultText": call["resultText"], "error": call["error"], "stack": call["stack"],
            "callbackInvocations": call["callbackInvocations"],
        }));
    }
    for error in value["xhrErrors"].as_array().into_iter().flatten() {
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "xhr-callback-error", "realm": realm, "where": error["where"],
            "state": error["state"], "status": error["status"],
            "responseTextLength": error["responseTextLength"],
            "message": error["message"], "stack": error["stack"],
        }));
    }
    for message in value["workerScopeMessages"].as_array().into_iter().flatten() {
        manifest.lock().unwrap().push(serde_json::json!({
            "kind": "worker-scope-message", "realm": realm,
            "started": message["started"], "ended": message["ended"],
            "handlerType": message["handlerType"], "isTrusted": message["isTrusted"],
            "origin": message["origin"], "source": message["source"],
            "error": message["error"], "stack": message["stack"],
        }));
    }
    for listener in value["listeners"].as_array().into_iter().flatten() {
        let source = listener["source"].as_str().unwrap_or_default();
        if source.is_empty() { continue }
        let url = format!("{}:{}", listener["target"].as_str().unwrap_or_default(),
            listener["type"].as_str().unwrap_or_default());
        write_source(dump_dir, manifest, next_file, "listener", &realm, &url, source);
    }
    for (index, shadow) in value["shadows"].as_array().into_iter().flatten().enumerate() {
        let source = shadow.as_str().unwrap_or_default();
        let name = format!("{realm}-shadow-{index}.html");
        let _ = std::fs::write(dump_dir.join(&name), source);
        manifest.lock().unwrap().push(serde_json::json!({
            "file": name, "kind": "shadow", "realm": realm, "bytes": source.len(),
        }));
    }
}

fn write_source(
    dump_dir: &std::path::Path,
    manifest: &Mutex<Vec<serde_json::Value>>,
    next_file: &AtomicUsize,
    kind: &str,
    realm: &str,
    url: &str,
    source: &str,
) {
    let number = next_file.fetch_add(1, Ordering::Relaxed);
    let name = format!("{number:04}-{kind}.js");
    if std::fs::write(dump_dir.join(&name), source).is_ok() {
        manifest.lock().unwrap().push(serde_json::json!({
            "file": name, "kind": kind, "realm": realm, "url": url, "bytes": source.len(),
        }));
    }
}
