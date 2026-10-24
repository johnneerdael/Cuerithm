import RecognitionSettings from "../components/RecognitionSettings";
import ProviderSetup,{providerSetupSummary} from "../components/ProviderSetup";
import type {ProviderSetupStatus} from "../provider-setup-types";
import {useCallback,useEffect,useRef,useState,type ReactNode} from "react";
import {ArrowUp,ArrowDown} from "lucide-react";
import type {AutotagConfig,Bridge,TagFrame,TaggingCatalog,TaggingOptions} from "../types";
import {errorMessage} from "../api";

function Row({label,note,children}:{label:string;note?:string;children:ReactNode}){
  return <label className="setting-row"><div><strong>{label}</strong>{note?<p>{note}</p>:null}</div>{children}</label>;
}
function Toggle({label,note,value,onChange}:{label:string;note?:string;value:boolean;onChange:(value:boolean)=>void}){
  return <Row label={label} note={note}><input aria-label={label} type="checkbox" checked={value} onChange={e=>onChange(e.target.checked)}/></Row>;
}
const fieldLabels:Record<string,string>={bpm:"BPM",isrc:"ISRC",url:"Catalog URL",albumArt:"Album art",otherTags:"Other provider tags",metaTags:"Tagging date marker",trackId:"Provider track ID",releaseId:"Provider release ID",catalogNumber:"Catalog number"};
const labelFor=(field:string)=>fieldLabels[field]??(field[0].toUpperCase()+field.slice(1).replace(/[A-Z]/g,c=>" "+c.toLowerCase()));
const routes=[["default","Genres and styles separately"],["onlyGenres","Genres only"],["onlyStyles","Styles only"],["mergeToGenres","Merge both into genres"],["mergeToStyles","Merge both into styles"],["stylesToGenre","Styles into genres"],["genresToStyle","Genres into styles"],["customTag","Styles into custom fields"]];
const swap=(values:string[],index:number,delta:number)=>{const next=[...values];[next[index],next[index+delta]]=[next[index+delta],next[index]];return next;};

export default function TaggingSettings({bridge,config,enabled,onChange,catalogRevision=0}:{catalogRevision?:number;bridge:Bridge;config:AutotagConfig|null|undefined;enabled:boolean;onChange:(config:AutotagConfig,enabled:boolean)=>void}){
  const [storage,setStorage]=useState<Awaited<ReturnType<Bridge["getTaggingStorage"]>>|null>(null);
  const [catalog,setCatalog]=useState<TaggingCatalog|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [retry,setRetry]=useState(0);
  const [accessError,setAccessError]=useState<string|null>(null);
  const [resetting,setResetting]=useState(false);
  const [setup,setSetup]=useState<ProviderSetupStatus[]>([]),[setupError,setSetupError]=useState<string|null>(null);
  const setupRequest=useRef(0),catalogRequest=useRef(0),mounted=useRef(true);
  const refreshSetup=useCallback(async()=>{const request=++setupRequest.current;try{const next=await bridge.providerSetupStatuses();if(mounted.current&&request===setupRequest.current){setSetup(next);setSetupError(null);}}catch{if(mounted.current&&request===setupRequest.current)setSetupError("Provider setup status is unavailable. Refresh before configuring an account.");}},[bridge]);
  const refreshCatalog=useCallback(async(showError:boolean)=>{const request=++catalogRequest.current;try{const next=await bridge.getTaggingCatalog();if(mounted.current&&request===catalogRequest.current){setCatalog(next);setError(null);}}catch(e){if(showError&&mounted.current&&request===catalogRequest.current)setError(errorMessage(e));}},[bridge]);
  useEffect(()=>{mounted.current=true;void refreshSetup();const listener=bridge.onCatalogAccessChange(()=>{void refreshSetup();void refreshCatalog(false);});return()=>{mounted.current=false;setupRequest.current++;catalogRequest.current++;void listener.then(dispose=>dispose());};},[bridge,refreshSetup,refreshCatalog]);
  useEffect(()=>{setError(null);void refreshCatalog(true);},[refreshCatalog,retry,catalogRevision]);
  useEffect(()=>{let active=true;bridge.getTaggingStorage().then(value=>{if(active)setStorage(value);}).catch(()=>{});return ()=>{active=false;};},[bridge]);
  if(error)return <section className="setting-section"><h2>Automatic tagging</h2><p role="alert">{error}</p><button className="secondary" onClick={()=>setRetry(n=>n+1)}>Retry loading tagging settings</button></section>;
  if(!catalog)return <section className="setting-section" aria-busy="true"><h2>Automatic tagging</h2><p className="setting-note">Loading tagging settings…</p></section>;
  const requiresSolver=(id:string)=>catalog.access?.some(a=>a.id===id&&a.requires_solver)??false;
  const defaultSolver=catalog.defaults.solver??{endpoint:null,timeout_seconds:120};
  const configuredSolver=config?.solver??defaultSolver;
  const hasSolver=!!configuredSolver.endpoint?.trim();
  const current=config??{...catalog.defaults,options:{...catalog.defaults.options,platforms:catalog.defaults.options.platforms.filter(id=>!requiresSolver(id)||hasSolver)}};
  const solver=current.solver??defaultSolver;
  const retainedCleanup=storage?[
    storage.pending_cleanup>0?`Pending cleanup: ${storage.pending_cleanup} ${storage.pending_cleanup===1?"operation":"operations"}.`:null,
    (storage.retained_cleanup_files??0)>0||(storage.retained_cleanup_bytes??0)>0?`Temporary files retained: ${storage.retained_cleanup_files??"count unavailable"} · ${storage.retained_cleanup_bytes===undefined?"size not reported":(storage.retained_cleanup_bytes/1048576).toFixed(1)+" MB measured"}.`:null,
    (storage.retained_cleanup_unknown_sizes??0)>0?`Size unavailable for ${storage.retained_cleanup_unknown_sizes} ${storage.retained_cleanup_unknown_sizes===1?"file":"files"}.`:null,
  ].filter(Boolean).join(" "):"";
  const resetAccess=async()=>{const request=++catalogRequest.current;setResetting(true);setAccessError(null);try{const next=await bridge.resetCatalogAccess();if(mounted.current&&request===catalogRequest.current)setCatalog(next);}catch(e){if(mounted.current)setAccessError(errorMessage(e));}finally{if(mounted.current)setResetting(false);}};
  const options=current.options;
  const update=(patch:Partial<AutotagConfig>)=>onChange({...current,...patch},enabled);
  const opt=(patch:Partial<TaggingOptions>)=>update({options:{...options,...patch}});
  const bool=(key:keyof TaggingOptions,label:string,note?:string)=><Toggle key={key} label={label} note={note} value={options[key]===true} onChange={value=>opt({[key]:value})}/>;
  const number=(key:keyof TaggingOptions,label:string,min:number,max:number,step=1,note?:string)=><Row key={key} label={label} note={note}><input aria-label={label} type="number" min={min} max={max} step={step} value={options[key] as number} onChange={e=>opt({[key]:Number(e.target.value)})}/></Row>;
  const text=(key:keyof TaggingOptions,label:string,note?:string)=><Row key={key} label={label} note={note}><input aria-label={label} type="text" value={(options[key] as string|null)??""} onChange={e=>opt({[key]:e.target.value||null})}/></Row>;
  const fields=[...new Set(catalog.providers.flatMap(p=>p.supportedTags).concat("metaTags"))];
  const ordered=[...options.platforms,...catalog.providers.map(p=>p.id).filter(id=>!options.platforms.includes(id))];
  const setList=(key:"tags"|"overwriteTags",field:string,value:boolean)=>opt({[key]:value?[...options[key],field]:options[key].filter(f=>f!==field)});
  const frame=(value:TagFrame|null,onFrame:(value:TagFrame)=>void)=>["id3","vorbis","mp4"].map(format=><Row key={format} label={"Style field · "+format.toUpperCase()}><input aria-label={"Style field · "+format.toUpperCase()} value={value?.[format as keyof TagFrame]??""} onChange={e=>onFrame({...value??{id3:"STYLE",vorbis:"STYLE",mp4:"STYLE"},[format]:e.target.value})}/></Row>);
  return <section className="setting-section tagging-settings">
    <div className="setting-heading"><div><h2>Automatic tagging</h2><p>Apply these settings to every incoming track after analysis.</p></div></div>
    <Toggle label="Automatic tagging" value={enabled} onChange={value=>onChange(current,value)} note="Catalog lookups use an internet connection. Analysis and cue generation work offline."/>
    <div className="setting-row"><div><strong>Recovery backups</strong><p>A copy of the audio is kept only while a write is under way. Undo restores the previous tags from the library, not from a copy of the file. Temporary files remain while cleanup is pending.</p>{retainedCleanup?<p>{retainedCleanup}</p>:null}</div><span className="fixed-setting">{storage?`${(storage.original_bytes/1048576).toFixed(1)} MB · ${storage.original_files} files`:"Storage unavailable"}</span></div>
    {setupError?<p role="alert" className="provider-setup-message">{setupError}<button className="back-link" onClick={()=>void refreshSetup()}>Refresh provider setup</button></p>:null}
    <div className="provider-order" aria-label="Catalog providers">{ordered.map(id=>{
      const provider=catalog.providers.find(p=>p.id===id);if(!provider)return null;
      const index=options.platforms.indexOf(id),selected=index>=0;
      const connection=setup.find(value=>value.id===id),needsSetup=["sonovault","bpmsupreme","volumo"].includes(id);
      const unavailable=requiresSolver(id)&&!hasSolver||(connection?!connection.can_use:needsSetup);
      return <div className="provider-entry" key={id}><div className="provider-line"><span className="provider-position">{selected?index+1:"—"}</span><label><input type="checkbox" aria-label={"Use "+provider.name} disabled={unavailable&&!selected} checked={selected} onChange={e=>opt({platforms:e.target.checked?[...options.platforms,id]:options.platforms.filter(p=>p!==id)})}/><strong>{provider.name}</strong></label><small>{requiresSolver(id)&&!hasSolver?"Unavailable · configure a solver":connection?providerSetupSummary(connection):requiresSolver(id)?"Uses configured solver":provider.maxThreads===1?"One request at a time":"Uses tagging concurrency"}</small><button aria-label={"Move "+provider.name+" earlier"} disabled={!selected||index===0} onClick={()=>opt({platforms:swap(options.platforms,index,-1)})}><ArrowUp size={15}/></button><button aria-label={"Move "+provider.name+" later"} disabled={!selected||index===options.platforms.length-1} onClick={()=>opt({platforms:swap(options.platforms,index,1)})}><ArrowDown size={15}/></button></div>{connection?<ProviderSetup bridge={bridge} name={provider.name} status={connection} onRefresh={refreshSetup}/>:null}</div>;
    })}</div>
    <RecognitionSettings config={current.recognition} configured={catalog.recognition?.acoustid_configured??false} onChange={recognition=>update({recognition})}/>
    <details className="tagging-group" open><summary>Cloudflare challenge solver</summary>
      <p className="setting-note">Optional. Only sources detected as blocked use this service. It receives their catalog URLs and search queries. Beatport keeps its direct catalog API connection. The connection is checked when used; an unreachable or incompatible solver leaves the source unavailable.</p>
      <Row label="Solver API URL" note="FlareSolverr, Byparr or Trawl. Enter the server URL or its /v1 endpoint; leave empty to disable."><input aria-label="Solver API URL" type="url" placeholder="http://localhost:8191" value={solver.endpoint??""} onChange={e=>update({solver:{...solver,endpoint:e.target.value||null}})}/></Row>
      <Row label="Solver timeout" note="Seconds per attempt. Initial challenges can take longer; default 120. Temporary failures are retried once."><input aria-label="Solver timeout" type="number" min={1} max={300} step={1} value={solver.timeout_seconds} onChange={e=>update({solver:{...solver,timeout_seconds:Number(e.target.value)}})}/></Row>
      <p className="setting-note">Detected restrictions are remembered across restarts. To test direct access again after a source or network changes, reset detected statuses.</p>
      <button className="secondary" type="button" disabled={resetting} onClick={()=>void resetAccess()}>{resetting?"Resetting source access…":"Retry direct source access"}</button>
      {accessError?<p role="alert">{accessError}</p>:null}
    </details>
    {bool("multiplatform","Combine providers","Off: use the first provider with a match. On: apply matches from every selected provider in order.")}
    <Row label="No catalog match"><select aria-label="No catalog match" value={current.no_match} onChange={e=>update({no_match:e.target.value as AutotagConfig["no_match"]})}><option value="keep_existing">Keep existing tags and finish</option><option value="attention">Keep in Incoming for attention</option></select></Row>
    <p className="setting-note">Provider errors keep the track in Incoming for retry. A successful preparation replaces cues and moves the track into Tracks.</p>
    <details className="tagging-group" open><summary>Matching</summary>
      {number("strictness","Matching strictness",0,1,0.01,"Higher values require a closer title and artist match.")}
      {number("threads","Tagging concurrency",1,64,1,"Shared across analysis workers. Provider-specific limits still apply.")}
      {bool("matchDuration","Match duration")}
      {number("maxDurationDifference","Maximum duration difference",0,600,1,"Seconds; used when duration matching is enabled.")}
      {bool("matchById","Match by embedded IDs","Use embedded provider IDs and ISRC when available.")}
      <Row label="Multiple catalog matches"><select aria-label="Multiple catalog matches" value={options.multipleMatches} onChange={e=>opt({multipleMatches:e.target.value as TaggingOptions["multipleMatches"]})}><option value="Default">Best match</option><option value="Oldest">Oldest release</option><option value="Newest">Newest release</option></select></Row>
      {bool("includeSubfolders","Include subfolders","Apply to watched folders and folders added to the library.")}
      {bool("parseFilename","Read artist and title from filename")}
      {text("filenameTemplate","Filename template","For example: %artists% - %title%. Other placeholders match unused parts.")}
      {text("titleRegex","Title cleanup pattern","A regular expression removed from the title before matching.")}
      {bool("skipTagged","Skip previously tagged catalog lookups","Recognizes OneTagger and CueRithm markers. Full audio analysis and replacement cues still run.")}
      {bool("albumTagging","Album matching","Use an identified Beatport or Traxsource release for the remaining incoming files in the same folder.")}
      {number("albumTaggingRatio","Album matching ratio",0,1,0.05,"Fraction of incoming files that must match one release before reusing its track list.")}
    </details>
    <details className="tagging-group" open><summary>Fields to write</summary>
      {bool("overwrite","Overwrite all selected fields","Turn off to keep existing values, with per-field exceptions below.")}
      <div className="tag-field-grid" role="group" aria-label="Metadata field policies"><div className="tag-field-head"><span>Field</span><span>Write</span><span>Overwrite</span></div>{fields.map(field=>{
        const label=labelFor(field);
        const supported=catalog.providers.filter(p=>options.platforms.includes(p.id)&&p.supportedTags.includes(field)).map(p=>p.name);
        return <div className="tag-field-row" key={field}><span>{label}<small>{field==="metaTags"?"CueRithm":supported.join(" · ")||"Not supplied by the selected providers"}</small></span><input type="checkbox" aria-label={"Write "+label} checked={options.tags.includes(field)} onChange={e=>setList("tags",field,e.target.checked)}/><input type="checkbox" aria-label={"Overwrite "+label} checked={options.overwrite||options.overwriteTags.includes(field)} disabled={options.overwrite||!options.tags.includes(field)} onChange={e=>setList("overwriteTags",field,e.target.checked)}/></div>;
      })}</div>
      <Row label="Key and BPM written to files" note="Applies to selected Key/BPM fields. The library always keeps its measured analysis."><select aria-label="Key and BPM written to files" value={current.analysis_tag_source} onChange={e=>update({analysis_tag_source:e.target.value as AutotagConfig["analysis_tag_source"]})}><option value="measured">Cuerithm analysis</option><option value="catalog">Catalog metadata</option></select></Row>
      {bool("camelot","Write Camelot keys")}
      <Toggle label="Write track energy to files" value={current.write_energy} onChange={value=>update({write_energy:value})} note="One whole-track energy value. Library analysis always retains it."/>
      {bool("shortTitle","Write short titles","Keep the mix/version out of the title field.")}
      {bool("onlyYear","Write year only")}
      {number("trackNumberLeadingZeroes","Track number padding",0,8,1,"Minimum digits, such as 2 for 01, 02…")}
    </details>
    <details className="tagging-group"><summary>Genres and styles</summary>
      <Row label="Genre and style routing"><select aria-label="Genre and style routing" value={options.stylesOptions} onChange={e=>opt({stylesOptions:e.target.value})}>{routes.map(([value,label])=><option key={value} value={value}>{label}</option>)}</select></Row>
      {bool("mergeGenres","Merge with existing genres")}{bool("capitalizeGenres","Capitalize genres")}
      {options.stylesOptions==="customTag"?frame(options.stylesCustomTag,value=>opt({stylesCustomTag:value})):null}
    </details>
    <details className="tagging-group"><summary>Artwork</summary>
      <p className="setting-note">Enable Album art in Fields to write to embed catalog artwork.</p>
      <Row label="Artwork destination"><select aria-label="Artwork destination" value={options.albumArtFile?"external":"embedded"} onChange={e=>opt({albumArtFile:e.target.value==="external"})}><option value="embedded">Embed in audio file</option><option value="external">External cover file</option></select></Row>
      {options.albumArtFile?<><p className="setting-note">Existing embedded covers are retained. Catalog artwork is saved beside the audio instead.</p>{text("coverFilename","Cover filename template","Optional; defaults to cover.jpg. Relative subfolders are supported.")}</>:bool("removeAllCovers","Replace all embedded covers","Used when Album art is selected for overwrite.")}
    </details>
    <details className="tagging-group"><summary>File tag format</summary>
      <Row label="ID3 version"><select aria-label="ID3 version" value={options.id3v24?"2.4":"2.3"} onChange={e=>opt({id3v24:e.target.value==="2.4"})}><option value="2.4">ID3v2.4</option><option value="2.3">ID3v2.3</option></select></Row>
      {text("id3CommLang","ID3 comment language","Optional three-letter code.")}
      {(["id3","vorbis","mp4"] as const).map(format=><Row key={format} label={"Value separator · "+format.toUpperCase()} note={format==="vorbis"?"Leave empty for separate native values.":undefined}><input aria-label={"Value separator · "+format.toUpperCase()} value={options.separators[format]??""} onChange={e=>opt({separators:{...options.separators,[format]:format==="vorbis"?(e.target.value||null):e.target.value}})}/></Row>)}
    </details>
    {catalog.providers.filter(p=>p.customOptions.options.length>0).map(provider=><details className="tagging-group" key={provider.id} open><summary>{provider.name} settings</summary>{provider.customOptions.options.map(option=>{
      const spec=option.value,value=options.custom[provider.id]?.[option.id];
      const note=provider.id==="beatport"&&option.id==="ignore_version"?"Ignore mix text while scoring titles. Known edition, recording ID and duration conflicts still require review.":option.tooltip??undefined;
      const set=(next:unknown)=>opt({custom:{...options.custom,[provider.id]:{...options.custom[provider.id],[option.id]:next}}});
      if(spec.type==="boolean")return <Toggle key={option.id} label={option.label} note={note} value={typeof value==="boolean"?value:spec.value} onChange={set}/>;
      if(spec.type==="number")return <Row key={option.id} label={option.label} note={option.tooltip??undefined}><input type="number" aria-label={option.label} min={spec.min} max={spec.max} step={spec.step} value={typeof value==="number"?value:spec.value} onChange={e=>set(Number(e.target.value))}/></Row>;
      if(spec.type==="string")return <Row key={option.id} label={option.label} note={option.tooltip??undefined}><input type={spec.hidden?"password":"text"} aria-label={option.label} value={typeof value==="string"?value:spec.value} onChange={e=>set(e.target.value)}/></Row>;
      if(spec.type==="option")return <Row key={option.id} label={option.label}><select aria-label={option.label} value={typeof value==="string"?value:spec.value} onChange={e=>set(e.target.value)}>{spec.values.map(v=><option key={v}>{v}</option>)}</select></Row>;
      return null;
    })}</details>)}
    <p className="setting-note tagging-license">Tagging uses OneTagger source under GPLv3. CueRithm desktop is GPLv3; the separate analysis engine remains MIT.</p>
  </section>;
}
