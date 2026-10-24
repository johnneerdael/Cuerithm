import {act,fireEvent,render,screen,waitFor,within} from "@testing-library/react";
import {useState} from "react";
import {beforeEach,describe,expect,it,vi} from "vitest";
import RekordboxSync from "../views/RekordboxSync";
import RekordboxEntities from "../components/RekordboxEntities";
import Sync from "../views/Sync";
import {fakeBridge} from "./bridge";
import {allowedDecisions,rekordboxValueText} from "../components/RekordboxConflicts";
import type {SyncTargetConfiguration} from "../sync-target-types";
import type {RekordboxBridge,RekordboxSyncFieldReview,RekordboxSyncPreview,RekordboxSyncTarget} from "../rekordbox-sync-types";
import {syncPreferencesKey} from "../sync-preferences";

beforeEach(()=>localStorage.removeItem(syncPreferencesKey));

const target:RekordboxSyncTarget={id:"native-6",installation:{version:"6.8.5",major:6,root:"/Applications/rekordbox 6",executable:"/Applications/rekordbox 6/rekordbox",version_source:"bundle"},database:"/Synthetic/rekordbox/master.db",options_file:"/Synthetic/options.json",asset_root:"/Synthetic/rekordbox"};
const field:RekordboxSyncFieldReview={track_id:"a",title:"Night drive",field:"title",local:{state:"present",value:"Night drive"},remote:{state:"present",value:"Night drive"},result:{state:"conflict",reason:"initial_acknowledgement",suggestion:"accept_equal"},decision:null};
function prepared(id="plan",fields=[field]):RekordboxSyncPreview{return {id,target_id:target.id,library_revision:1,fields,entity_changes:[],pending_entities:[],tracks:1,changed_rows:2,skipped_fields:0,warnings:[],media_copies:0,copied_audio_bytes:0,can_apply:fields.every(f=>f.decision!==null)};}
function bridge():RekordboxBridge{return {configure:vi.fn(async()=>target),syncTargetConfigurations:vi.fn(async()=>[]),saveSyncTargetConfiguration:vi.fn(async request=>({id:request.id??"saved",name:request.name,revision:(request.expected_revision??0)+1,target:request.target})),deleteSyncTargetConfiguration:vi.fn(async()=>{}),discover:vi.fn(async()=>[target]),preflight:vi.fn(async()=>({target,readable:true,writable:true,restrictions:[],schema_fingerprint:null,identity:null})),preview:vi.fn(async()=>prepared()),apply:vi.fn(async(_op,id)=>({id,status:"applied",tracks:1,changed_rows:2,acknowledged_fields:1,skipped_fields:1})),restore:vi.fn(async(_op,id)=>({id,status:"restored",tracks:1,changed_rows:2,acknowledged_fields:0,skipped_fields:0})),recover:vi.fn(async(_op,id)=>({id,status:"applied",tracks:1,changed_rows:2,acknowledged_fields:1,skipped_fields:0})),discard:vi.fn(async()=>{}),history:vi.fn(async()=>[]),cancel:vi.fn(async()=>true)};}
async function open(api:RekordboxBridge){render(<RekordboxSync bridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);await screen.findByText("Ready for preview");}
describe("native rekordbox sync",()=>{
 it("checks the selected saved revision and clears that binding when switching installed libraries",async()=>{
  const api=bridge(),other={...target,id:"native-7",installation:{...target.installation,version:"7.2.0",major:7}};
  vi.mocked(api.discover).mockResolvedValue([target,other]);
  vi.mocked(api.syncTargetConfigurations).mockResolvedValue([{id:"authored-setup",name:"Saved decks",revision:4,target:{kind:"rekordbox",target}}]);
  vi.mocked(api.preflight).mockImplementation(async(_op,id)=>({target:id===other.id?other:target,readable:true,writable:true,restrictions:[],schema_fingerprint:null,identity:null}));
  await open(api);await screen.findByRole("option",{name:"Saved decks"});
  fireEvent.change(screen.getByRole("combobox",{name:"Saved library setup"}),{target:{value:"authored-setup"}});
  await waitFor(()=>expect(api.preflight).toHaveBeenCalledWith(expect.any(String),target.id,{id:"authored-setup",revision:4}));
  fireEvent.change(screen.getByRole("combobox",{name:"rekordbox library"}),{target:{value:other.id}});
  await waitFor(()=>expect(vi.mocked(api.preflight).mock.lastCall?.slice(1)).toEqual([other.id]));
  expect(screen.getByRole("combobox",{name:"Saved library setup"})).toHaveValue("");
  await waitFor(()=>expect(screen.getByRole("button",{name:"Preview rekordbox sync"})).toBeEnabled());
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  await waitFor(()=>expect(api.preview).toHaveBeenCalled());
  expect(vi.mocked(api.preview).mock.lastCall?.[1]).toMatchObject({target_id:other.id});
  expect(vi.mocked(api.preview).mock.lastCall?.[1]).not.toHaveProperty("configuration");
 });
 it("saves a manually selected Windows library without starting a sync",async()=>{
  const api=bridge();await open(api);
  fireEvent.click(screen.getByText("Set up another library"));
  fireEvent.change(screen.getByRole("textbox",{name:"Library setup name"}),{target:{value:"Proton rekordbox"}});
  for(const [name,value] of [["rekordbox executable","/prefix/drive_c/rekordbox.exe"],["rekordbox database","/private/master.db"],["rekordbox Agent options","/private/options.json"],["rekordbox asset folder","/private/share"]])fireEvent.change(screen.getByRole("textbox",{name}),{target:{value}});
  fireEvent.change(screen.getByRole("combobox",{name:"Application environment"}),{target:{value:"windows_compatibility"}});
  fireEvent.change(screen.getByRole("textbox",{name:"Wine or Proton prefix"}),{target:{value:"/prefix"}});
  fireEvent.click(screen.getByRole("button",{name:"Save library setup"}));
  await waitFor(()=>expect(api.configure).toHaveBeenCalledWith(expect.objectContaining({executable:"/prefix/drive_c/rekordbox.exe",database:"/private/master.db",environment:{kind:"windows_compatibility",prefix:"/prefix",installation:null,mappings:[]}})));
  await waitFor(()=>expect(api.saveSyncTargetConfiguration).toHaveBeenCalledWith(expect.objectContaining({name:"Proton rekordbox",target:{kind:"rekordbox",target}})));
  expect(api.preview).not.toHaveBeenCalled();expect(api.apply).not.toHaveBeenCalled();
 });
 it("keeps a newly saved executable bound when its library ID matches an older setup",async()=>{
  const api=bridge(),replacement={...target,installation:{...target.installation,version:"6.8.6",root:"/Synthetic/rekordbox-new",executable:"/Synthetic/rekordbox-new/rekordbox"}};
  const older:SyncTargetConfiguration={id:"older-setup",name:"Older executable",revision:2,target:{kind:"rekordbox",target}};
  const saved:SyncTargetConfiguration={id:"new-setup",name:"New executable",revision:1,target:{kind:"rekordbox",target:replacement}};
  let finishRefresh!:(values:SyncTargetConfiguration[])=>void;
  vi.mocked(api.configure).mockResolvedValue(replacement);
  vi.mocked(api.saveSyncTargetConfiguration).mockResolvedValue(saved);
  vi.mocked(api.syncTargetConfigurations).mockResolvedValueOnce([older]).mockImplementationOnce(()=>new Promise(resolve=>{finishRefresh=resolve;}));
  vi.mocked(api.preflight).mockImplementation(async(_op,_id,configuration)=>({target:configuration?.id===saved.id?replacement:target,readable:true,writable:true,restrictions:[],schema_fingerprint:null,identity:null}));
  await open(api);await screen.findByRole("option",{name:"Older executable"});
  fireEvent.change(screen.getByRole("combobox",{name:"Saved library setup"}),{target:{value:older.id}});
  await waitFor(()=>expect(vi.mocked(api.preflight).mock.lastCall?.[2]).toEqual({id:older.id,revision:2}));
  await waitFor(()=>expect(screen.getByRole("button",{name:"Preview rekordbox sync"})).toBeEnabled());
  fireEvent.click(screen.getByText("Set up another library"));
  fireEvent.change(screen.getByRole("textbox",{name:"Library setup name"}),{target:{value:saved.name}});
  for(const [name,value] of [["rekordbox executable",replacement.installation.executable],["rekordbox database",replacement.database],["rekordbox Agent options",replacement.options_file],["rekordbox asset folder",replacement.asset_root]])fireEvent.change(screen.getByRole("textbox",{name}),{target:{value}});
  fireEvent.click(screen.getByRole("button",{name:"Save library setup"}));
  await waitFor(()=>expect(api.syncTargetConfigurations).toHaveBeenCalledTimes(2));
  await waitFor(()=>expect(vi.mocked(api.preflight).mock.lastCall?.slice(1)).toEqual([target.id,{id:saved.id,revision:1}]));
  await act(async()=>finishRefresh([older,saved]));
  await waitFor(()=>expect(screen.getByRole("combobox",{name:"Saved library setup"})).toHaveValue(saved.id));
  expect(screen.getByRole("textbox",{name:"Setup name"})).toHaveValue(saved.name);
  await waitFor(()=>expect(screen.getByRole("button",{name:"Preview rekordbox sync"})).toBeEnabled());
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  await waitFor(()=>expect(api.preview).toHaveBeenCalledWith(expect.any(String),expect.objectContaining({target_id:target.id,configuration:{id:saved.id,revision:1}})));
 });
 it("requires an explicit equal acknowledgement and a new server preview before applying its ID",async()=>{
  const api=bridge();await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  await screen.findByRole("button",{name:"Accept equal values"});expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"Accept equal values"}));await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("plan"));
  expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
  vi.mocked(api.preview).mockResolvedValueOnce(prepared("reviewed",[{...field,decision:"accept_equal"}]));
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled());
  expect(vi.mocked(api.preview).mock.calls[1][1].decisions).toEqual({a:{title:"accept_equal"}});
  fireEvent.click(screen.getByRole("button",{name:"Sync rekordbox"}));await waitFor(()=>expect(api.apply).toHaveBeenCalledWith(expect.any(String),"reviewed"));
  await screen.findByText("1 track · 2 rows changed · 1 field acknowledged · 1 field skipped");
  expect(screen.queryByText("Everything is already synced.")).not.toBeInTheDocument();
 });
 it("explains missing playlist members and includes them after enabling title metadata",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValueOnce({...prepared("pending",[]),can_apply:false,pending_entities:[{kind:"playlist",name:"Opening set",unresolved_members:1,member_titles:["Night drive"]}],entity_changes:[]});await open(api);
  fireEvent.click(screen.getByText("Metadata fields"));
  fireEvent.click(screen.getByRole("checkbox",{name:"title"}));
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  const entities=await screen.findByRole("region",{name:"rekordbox playlist and My Tag review"});
  expect(within(entities).getByText("Opening set")).toBeInTheDocument();expect(within(entities).getByText(/1 member missing from this preview/)).toBeInTheDocument();expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
  expect(within(entities).queryByText(/choose Use CueRithm/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox",{name:"title"}));
  expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
  vi.mocked(api.preview).mockResolvedValueOnce({...prepared("complete",[{...field,new_track:true,remote:{state:"unavailable"},result:{state:"proposed",decision:"use_local"},decision:"use_local"}]),pending_entities:[],entity_changes:[{local_id:"private-local-id",kind:"playlist",action:"create",ownership:"new",materialized_smart:true,before:null,after:{name:"Opening set",parent_path:["Weekend"],representation:"ordinary",position:2,member_count:1,members:[{title:"Night drive",position:1,external:false}],external_member_count:0}}]});
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByText("Smart playlist exported as a track list");expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled();expect(vi.mocked(api.preview).mock.calls[1][1].decisions).toEqual({});
  expect(vi.mocked(api.preview).mock.calls[1][1].options.metadata_fields).toContain("title");
  expect(screen.getByRole("region",{name:"rekordbox playlist and My Tag review"})).not.toHaveTextContent("private-local-id");
 });
 it("shows readable profiles without enabling writes",async()=>{
  const api=bridge();vi.mocked(api.preflight).mockResolvedValue({target,readable:true,writable:false,restrictions:["This installed build has not passed write acceptance."],schema_fingerprint:null,identity:null});
  render(<RekordboxSync bridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);await screen.findByText("Read only");
  expect(screen.getByText("This installed build has not passed write acceptance.")).toBeInTheDocument();
  vi.mocked(api.preview).mockResolvedValue(prepared("read-only",[{...field,decision:"accept_equal"}]));fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  await screen.findByRole("region",{name:"rekordbox field review"});expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();expect(api.apply).not.toHaveBeenCalled();
 });
 it("discards a late preview when scope changes",async()=>{
  const api=bridge();let finish!:(p:RekordboxSyncPreview)=>void;vi.mocked(api.preview).mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));await open(api);
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(api.preview).toHaveBeenCalled());
  fireEvent.change(screen.getByRole("combobox",{name:"rekordbox scope"}),{target:{value:"changed"}});await act(async()=>finish(prepared("late")));
  await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("late"));expect(screen.queryByRole("region",{name:"rekordbox field review"})).not.toBeInTheDocument();
 });
 it("retains reviewed choices after a stale apply error and requires another preview",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("stale",[{...field,decision:"accept_equal"}]));vi.mocked(api.apply).mockRejectedValue({code:"stale_preview",message:"rekordbox changed. Preview again."});await open(api);
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled());fireEvent.click(screen.getByRole("button",{name:"Sync rekordbox"}));
  await screen.findByRole("alert");expect(screen.getByText("rekordbox changed. Preview again.")).toBeInTheDocument();expect(screen.getByRole("region",{name:"rekordbox field review"})).toBeInTheDocument();expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
 });
 it("keeps unavailable and empty observations distinct and offers explicit remote clearing",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("empty",[{...field,field:"comment",local:{state:"present",value:null},remote:{state:"unavailable"},result:{state:"conflict",reason:"observation_unavailable",suggestion:null}},{...field,field:"genre",local:{state:"present",value:null},remote:{state:"present",value:"House"}}]));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  const review=await screen.findByRole("region",{name:"rekordbox field review"});expect(within(review).getByText("Unavailable")).toBeInTheDocument();expect(within(review).getAllByText("Empty")).toHaveLength(2);expect(within(review).getByRole("option",{name:"Clear in rekordbox"})).toBeInTheDocument();expect(within(review).queryByRole("option",{name:"Use CueRithm"})).not.toBeInTheDocument();
 });
 it("recovers interrupted history and requires confirmation before guarded restore",async()=>{
  const api=bridge();vi.mocked(api.history).mockResolvedValue([{id:"interrupted",target_id:target.id,status:"native_published",created:1700000000000,updated:1700000000000,outcome:null},{id:"done",target_id:target.id,status:"applied",created:1700000000000,updated:1700000000000,outcome:null}]);await open(api);
  fireEvent.click(screen.getByRole("button",{name:"Recover sync"}));await waitFor(()=>expect(api.recover).toHaveBeenCalledWith(expect.any(String),"interrupted"));fireEvent.click(screen.getAllByRole("button",{name:"Restore backup"})[0]);expect(api.restore).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"Confirm restore"}));await waitFor(()=>expect(api.restore).toHaveBeenCalledWith(expect.any(String),"interrupted"));
 });
 it("lists a sync whose backup was removed without offering to restore it",async()=>{
  const api=bridge();vi.mocked(api.history).mockResolvedValue([{id:"old",target_id:target.id,status:"applied",created:1700000000000,updated:1700000000000,outcome:null,restore_available:false},{id:"new",target_id:target.id,status:"applied",created:1700000001000,updated:1700000001000,outcome:null,restore_available:true}]);await open(api);
  await screen.findByText(/The backup was removed after ten newer syncs of this database/);
  expect(screen.getAllByRole("button",{name:"Restore backup"})).toHaveLength(1);
 });
 it("cancels active work without releasing the operation until it settles",async()=>{
  const api=bridge();let finish!:(p:RekordboxSyncPreview)=>void;vi.mocked(api.preview).mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  await waitFor(()=>expect(api.preview).toHaveBeenCalled());const op=vi.mocked(api.preview).mock.calls[0][0];fireEvent.click(screen.getByRole("button",{name:"Cancel"}));await waitFor(()=>expect(api.cancel).toHaveBeenCalledWith(op));expect(screen.getByRole("button",{name:"Preview rekordbox sync"})).toBeDisabled();await act(async()=>finish(prepared("cancelled")));await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("cancelled"));
 });
 it("reports unavailable desktop support when no native installation is discovered",async()=>{
  const api=bridge();vi.mocked(api.discover).mockResolvedValue([]);render(<RekordboxSync bridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);await screen.findByText("No rekordbox library found. Select a native installation or configure its Windows library for Wine / Proton below.");expect(api.preflight).not.toHaveBeenCalled();expect(screen.getByRole("button",{name:"Preview rekordbox sync"})).toBeDisabled();
 });
 it("bulk skips only unresolved fields and retains explicit choices",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("conflicts",[field,{...field,field:"artist",local:{state:"present",value:"Artist A"},remote:{state:"present",value:"Artist B"}}]));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByRole("region",{name:"rekordbox field review"});
  fireEvent.click(screen.getByRole("button",{name:"Accept equal values"}));fireEvent.click(screen.getByRole("button",{name:"Skip unresolved fields"}));
  expect(screen.getByRole("combobox",{name:"Choice for Night drive title"})).toHaveValue("accept_equal");expect(screen.getByRole("combobox",{name:"Choice for Night drive artist"})).toHaveValue("skip");
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(api.preview).toHaveBeenCalledTimes(2));expect(vi.mocked(api.preview).mock.calls[1][1].decisions).toEqual({a:{title:"accept_equal",artist:"skip"}});
 });
 it("invalidates prepared fields when the destination or library revision changes",async()=>{
  const api=bridge(),other={...target,id:"native-7",installation:{...target.installation,version:"7.2.0",major:7}};vi.mocked(api.discover).mockResolvedValue([target,other]);vi.mocked(api.preflight).mockImplementation(async(_op,id)=>({target:id===other.id?other:target,readable:true,writable:true,restrictions:[],schema_fingerprint:null,identity:null}));vi.mocked(api.preview).mockResolvedValue(prepared("target-plan",[{...field,decision:"accept_equal"}]));
  const view=render(<RekordboxSync bridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);await screen.findByText("Ready for preview");fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled());
  fireEvent.change(screen.getByRole("combobox",{name:"rekordbox library"}),{target:{value:other.id}});await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("target-plan"));expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();await screen.findByText("Ready for preview");
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled());view.rerender(<RekordboxSync bridge={api} workspace={null} revision={2} onChanged={()=>{}}/>);await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled());expect(api.apply).not.toHaveBeenCalled();
 });
 it("rechecks availability after closing an open Agent",async()=>{
  const api=bridge();vi.mocked(api.preflight).mockResolvedValueOnce({target,readable:true,writable:false,restrictions:["rekordbox Agent is running. Close it before syncing."],schema_fingerprint:null,identity:null});render(<RekordboxSync bridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);await screen.findByText("rekordbox Agent is running. Close it before syncing.");
  fireEvent.click(screen.getByRole("button",{name:"Refresh rekordbox libraries"}));await screen.findByText("Ready for preview");expect(api.preflight).toHaveBeenCalledTimes(2);
 });
 it("offers rekordbox alongside the other destinations and restores it after remount",async()=>{
  const api=bridge(),adapters=fakeBridge([]);
  const view=render(<Sync bridge={adapters} rekordboxBridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);
  expect(within(screen.getByRole("combobox",{name:"Sync to"})).getAllByRole("option")).toHaveLength(7);expect(api.discover).not.toHaveBeenCalled();
  expect(screen.queryByRole("navigation",{name:"Sync destinations"})).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("combobox",{name:"Sync to"}),{target:{value:"rekordbox"}});
  await screen.findByText("Ready for preview");expect(screen.getByRole("combobox",{name:"Sync to"})).toHaveValue("rekordbox");
  expect(adapters.previewDjSync).not.toHaveBeenCalled();
  fireEvent.change(screen.getByRole("combobox",{name:"Sync to"}),{target:{value:"virtual_dj"}});
  fireEvent.change(screen.getByRole("textbox",{name:"VirtualDJ library folder"}),{target:{value:"/SavedDJ"}});
  fireEvent.change(screen.getByRole("combobox",{name:"Sync to"}),{target:{value:"rekordbox"}});
  await screen.findByText("Ready for preview");view.unmount();
  render(<Sync bridge={adapters} rekordboxBridge={api} workspace={null} revision={1} onChanged={()=>{}}/>);
  await screen.findByText("Ready for preview");expect(screen.getByRole("combobox",{name:"Sync to"})).toHaveValue("rekordbox");
  fireEvent.change(screen.getByRole("combobox",{name:"Sync to"}),{target:{value:"virtual_dj"}});
  expect(screen.getByRole("textbox",{name:"VirtualDJ library folder"})).toHaveValue("/SavedDJ");
 });
 it("shows new tracks as automatic additions without field choices and applies the first preview",async()=>{
  const api=bridge();
  const newFields:RekordboxSyncFieldReview[]=[{...field,new_track:true,remote:{state:"unavailable"},result:{state:"proposed",decision:"use_local"},decision:"use_local"},{...field,new_track:true,field:"disc_number",local:{state:"present",value:null},remote:{state:"unavailable"},result:{state:"unchanged"},decision:null}];
  vi.mocked(api.preview).mockResolvedValue({...prepared("new-track",newFields),can_apply:true});await open(api);
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));
  const additions=await screen.findByRole("region",{name:"New rekordbox tracks"});
  expect(additions).toHaveTextContent("Night drive");expect(additions).toHaveTextContent("1 new track");
  expect(screen.queryByRole("combobox",{name:/Choice for Night drive/})).not.toBeInTheDocument();
  expect(screen.queryByText(/fields need a choice/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled();
  fireEvent.click(screen.getByRole("button",{name:"Sync rekordbox"}));
  await waitFor(()=>expect(api.apply).toHaveBeenCalledWith(expect.any(String),"new-track"));
  expect(vi.mocked(api.preview).mock.calls).toHaveLength(1);
  expect(vi.mocked(api.preview).mock.calls[0][1].decisions).toEqual({});
 });
 it("uses explicit bulk values only where available and can reset the choices",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("new",[{...field,remote:{state:"unavailable"}},{...field,field:"comment",local:{state:"present",value:null},remote:{state:"present",value:null}}]));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByRole("region",{name:"rekordbox field review"});
  fireEvent.click(screen.getByRole("button",{name:"Use CueRithm for unresolved"}));expect(screen.getByRole("combobox",{name:"Choice for Night drive title"})).toHaveValue("use_local");expect(screen.getByRole("combobox",{name:"Choice for Night drive comment"})).toHaveValue("");
  fireEvent.click(screen.getByRole("button",{name:"Keep rekordbox for unresolved"}));expect(screen.getByRole("combobox",{name:"Choice for Night drive title"})).toHaveValue("use_local");expect(screen.getByRole("combobox",{name:"Choice for Night drive comment"})).toHaveValue("use_remote");
  fireEvent.click(screen.getByRole("button",{name:"Reset choices"}));expect(screen.getByRole("combobox",{name:"Choice for Night drive title"})).toHaveValue("");expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
 });
 it("waits for a cancelled preview before preflighting another destination",async()=>{
  const api=bridge(),other={...target,id:"other"};vi.mocked(api.discover).mockResolvedValue([target,other]);let finish!:(p:RekordboxSyncPreview)=>void;vi.mocked(api.preview).mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(api.preview).toHaveBeenCalled());
  fireEvent.change(screen.getByRole("combobox",{name:"rekordbox library"}),{target:{value:other.id}});expect(api.preflight).toHaveBeenCalledTimes(1);await act(async()=>finish(prepared("old-destination")));await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("old-destination"));await waitFor(()=>expect(api.preflight).toHaveBeenCalledTimes(2));expect(api.apply).not.toHaveBeenCalled();
 });
 it("retains actual completion counts when the parent refreshes the library revision",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("complete",[{...field,decision:"accept_equal"}]));
  function Workspace(){const [revision,setRevision]=useState(1);return <RekordboxSync bridge={api} workspace={null} revision={revision} onChanged={()=>setRevision(value=>value+1)}/>;}
  render(<Workspace/>);await screen.findByText("Ready for preview");fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeEnabled());fireEvent.click(screen.getByRole("button",{name:"Sync rekordbox"}));await screen.findByText("1 track · 2 rows changed · 1 field acknowledged · 1 field skipped");await screen.findByText("Ready for preview");expect(screen.getByText("1 track · 2 rows changed · 1 field acknowledged · 1 field skipped")).toBeInTheDocument();expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
 });
 it("never converts a server cue proposal into an explicit ownership transfer when another field changes",async()=>{
  const api=bridge();vi.mocked(api.preview).mockResolvedValue(prepared("proposed",[field,{...field,field:"cues",local:{state:"present",value:[]},remote:{state:"present",value:[]},result:{state:"proposed",decision:"use_local"},decision:"use_local"}]));await open(api);fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByRole("region",{name:"rekordbox field review"});
  fireEvent.change(screen.getByRole("combobox",{name:"Choice for Night drive title"}),{target:{value:"use_remote"}});fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await waitFor(()=>expect(api.preview).toHaveBeenCalledTimes(2));expect(vi.mocked(api.preview).mock.calls[1][1].decisions).toEqual({a:{title:"use_remote"}});
 });
 it("requires explicit compatible audio and nearest color options, then reviews copy count and space",async()=>{
  const api=bridge();await open(api);
  expect(screen.getByRole("checkbox",{name:"Create compatible WAV copies for unsupported audio"})).not.toBeChecked();expect(screen.getByRole("checkbox",{name:"Use nearest supported cue colors"})).not.toBeChecked();
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByRole("region",{name:"rekordbox field review"});
  expect(vi.mocked(api.preview).mock.calls[0][1].options).toMatchObject({convert_incompatible_audio:false,nearest_color:false});
  fireEvent.click(screen.getByRole("checkbox",{name:"Create compatible WAV copies for unsupported audio"}));await waitFor(()=>expect(api.discard).toHaveBeenCalledWith("plan"));expect(screen.getByRole("button",{name:"Sync rekordbox"})).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox",{name:"Use nearest supported cue colors"}));vi.mocked(api.preview).mockResolvedValueOnce({...prepared("copies"),media_copies:2,copied_audio_bytes:10485760});
  fireEvent.click(screen.getByRole("button",{name:"Preview rekordbox sync"}));await screen.findByText("2 compatible audio copies · 10.0 MB of additional library storage");
  expect(vi.mocked(api.preview).mock.calls[1][1].options).toMatchObject({convert_incompatible_audio:true,nearest_color:true});expect(screen.getByText("Copies leave your source files unchanged. WAV copies use extra space in the rekordbox library; the preview shows the required size.")).toBeInTheDocument();
 });
});

describe("rekordbox musical field presentation",()=>{
 it("formats native cues in seconds and excludes native identity and raw payload fields",()=>{
  const text=rekordboxValueText("cues",{state:"present",value:[{id:"private-native-id",uuid:"private-uuid",raw_hash:"private-raw-hash",status:9,kind:4,slot:null,memory:true,start_seconds:65.25,end_seconds:67.5,active_loop:true,comment:"Intro"}]});
  expect(text).toBe("Memory loop · Intro · 1:05.250 – 1:07.500 · Active loop");expect(text).not.toMatch(/private|raw_hash|uuid|status/);
 });
 it("formats publication cue aliases on their source seconds clock and preserves memory cues",()=>{
  expect(rekordboxValueText("cues",{state:"present",value:[{name:"Drop",startTime:0.125,endTime:2.125,type:"5",position:1,activeLoop:true,data:{rekordbox:{originalNum:-1},internalId:"secret"}},{name:"Break",startTime:130.5,type:"1",position:1}]})).toBe("Memory loop · Drop · 0:00.125 – 0:02.125 · Active loop\nHot B · Break · 2:10.500");
 });
 it("formats sparse local grid anchors and dense native milliseconds without changing their clocks",()=>{
  expect(rekordboxValueText("beat_grid",{state:"present",value:[{seconds:0.125,bpm:128,beat:1},{seconds:60.25,bpm:130,beat:129}]})).toBe("128.00–130.00 BPM · 2 anchors\nBeat 1 at 0:00.125\nBeat 129 at 1:00.250");
  expect(rekordboxValueText("beat_grid",{state:"present",value:[{time_ms:125,tempo:12800,number:1},{time_ms:594,tempo:12800,number:2}]})).toBe("128.00 BPM · 2 beats\nBeat 1 at 0:00.125");
 });
 it("shows artwork availability and text arrays without hashes or raw unknown values",()=>{
  expect(rekordboxValueText("artwork",{state:"present",value:"secret-artwork-sha256"})).toBe("Artwork available");expect(rekordboxValueText("artist",{state:"present",value:["Artist A","Artist B"]})).toBe("Artist A; Artist B");expect(rekordboxValueText("title",{state:"present",value:{privateId:"secret"}})).toBe("Present · details unavailable");expect(rekordboxValueText("title",{state:"present",value:null})).toBe("Empty");expect(rekordboxValueText("title",{state:"unavailable"})).toBe("Unavailable");
 });
 it("labels explicit cue replacement and clearing and never offers clearing a full beatgrid or source path",()=>{
  const cues=allowedDecisions({...field,field:"cues"});expect(cues).toContainEqual(["use_local","Replace listed rekordbox cues"]);expect(cues).toContainEqual(["clear_remote","Clear listed rekordbox cues"]);for(const name of ["beat_grid","source_path"])expect(allowedDecisions({...field,field:name}).some(([decision])=>decision==="clear_remote")).toBe(false);
 });
});


describe("rekordbox entity presentation",()=>{
 it("shows owned deletion, names, hierarchy, positions and retained foreign memberships without identifiers",()=>{
  const before={name:"Warm up",parent_path:["Friday"],representation:"ordinary" as const,position:1,member_count:2,external_member_count:1,members:[{title:"External song",position:1,external:true},{title:"Night drive",position:2,external:false}]};
  render(<RekordboxEntities pending={[]} changes={[
   {local_id:"secret-identity",kind:"playlist",action:"update",ownership:"matched",materialized_smart:false,before,after:{...before,name:"Closing",parent_path:["Saturday"],position:3,members:[{title:"External song",position:1,external:true},{title:"Night drive",position:4,external:false}]}},
   {local_id:"secret-removal",kind:"playlist",action:"remove",ownership:"owned",materialized_smart:false,before:{...before,name:"Old set"},after:null},
   {local_id:"secret-category",kind:"category",action:"update",ownership:"owned",materialized_smart:false,before:{...before,name:"Mood",members:[],member_count:0},after:{...before,name:"Atmosphere",members:[],member_count:0}},
   {local_id:"secret-tag",kind:"tag",action:"update",ownership:"owned",materialized_smart:false,before:{...before,name:"Warm",member_count:0,members:[]},after:{...before,name:"Warm",parent_path:["Atmosphere"],member_count:1,members:[{title:"Night drive",position:null,external:false}]}}
  ]}/>);
  const region=screen.getByRole("region",{name:"rekordbox playlist and My Tag review"});
  for(const name of ["Warm up","Closing","Library root / Friday · Position 1","Library root / Saturday · Position 3","Delete owned object · Playlist","Deleted from rekordbox","Mood","Atmosphere","Update · My Tag category","Update · My Tag"])expect(within(region).getAllByText(name).length).toBeGreaterThan(0);
  expect(within(region).getAllByText(/external member retained/).length).toBeGreaterThan(0);expect(region).not.toHaveTextContent(/secret-|UUID|raw_hash|\.db/);
 });
});
