import {fireEvent,render,screen,waitFor,within} from "@testing-library/react";
import {beforeEach,describe,expect,it,vi} from "vitest";
import Maintenance from "../views/Maintenance";
import type {MaintenanceBridge} from "../maintenance-api";
import type {MissingRepairBridge} from "../missing-repair-api";
import type {BrokenScanReport,MaintenanceFileState} from "../maintenance-types";
import type {MissingPreview,MissingRepairEntry,MissingRow} from "../missing-repair-types";

const stamp={inputs:{input_revision:1,playlist_revision:1,preference_revision:1},maintenance_revision:1};
const row=(id:string,state:MaintenanceFileState,extra:Record<string,unknown>={})=>({id,display_path:`/Music/${id}.flac`,state,detail:null,observed_sha256:null,source_changed:false,...extra});
const scan:BrokenScanReport={id:"scan",created:1,stamp,stale:false,stale_reason:null,request:{inspect_audio:false,include_archived:true},decoder_identity:null,tracks:[row("gone-a","missing"),row("gone-b","missing",{archived:true}),row("drive","unavailable"),row("here","present")]};
const previewRow=(id:string,extra:Partial<MissingRow>={}):MissingRow=>({track_id:id,title:"Title "+id,artist:"Artist",archived:false,display_path:`/Music/${id}.flac`,file_state:"missing",outcome:"ready",reason:null,new_path:null,playlists:0,sync_links:0,rekordbox_links:0,...extra});
const impact={playlists:[{id:"p",name:"Warm up",tracks:2}],playlist_memberships:2,sync_links:1,rekordbox_links:1,archived:1,unreachable:0};
const entry=(kind:"remove"|"relink",extra:Partial<MissingRepairEntry>={}):MissingRepairEntry=>({id:"entry",kind,preview_id:"preview",created:1,undo_of:null,track_count:1,tracks:[{id:"gone-a",title:"Title gone-a",artist:"Artist",archived:false,display_path:"/Music/gone-a.flac",new_path:kind==="relink"?"/Moved/gone-a.flac":null}],skipped:[],playlist_ids:[],playlist_memberships:2,sync_links:1,rekordbox_links:1,stamp,...extra});
function bridges(){
  const api={onProgress:vi.fn(async()=>()=>{}),onChange:vi.fn(async()=>()=>{}),cancel:vi.fn(async()=>true),scanBroken:vi.fn(async()=>scan),brokenScan:vi.fn(async()=>scan),recovery:vi.fn(async()=>[]),mergeHistory:vi.fn(async()=>[]),unusedRemovalHistory:vi.fn(async()=>[])} as unknown as MaintenanceBridge;
  const removal=(ids:string[]):MissingPreview=>({id:"preview",kind:"remove",scan_id:"scan",created:1,rows:ids.map(id=>previewRow(id,id==="drive"?{file_state:"unavailable"}:{})),ready:ids.length,impact:{...impact,unreachable:ids.includes("drive")?1:0},locations:[],files_checked:0});
  const repair={
    previewRemoval:vi.fn(async(_op:string,_scan:string,ids:string[])=>removal(ids)),
    applyRemoval:vi.fn(async()=>entry("remove")),
    previewRelink:vi.fn(async(_op:string,_scan:string,ids:string[])=>({...removal(ids),kind:"relink" as const,rows:ids.map(id=>previewRow(id,{new_path:"/Moved/"+id+".flac"})),locations:["/Moved"],files_checked:7})),
    applyRelink:vi.fn(async()=>entry("relink")),
    history:vi.fn(async()=>[]),
    undo:vi.fn(async()=>entry("remove",{undo_of:"entry"})),
    discardUndo:vi.fn(async()=>({entry:entry("remove"),undone_by:null,undo_discarded:true})),
  } as unknown as MissingRepairBridge;
  return {api,repair};
}
async function scanned(b=bridges()){
  render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>"/Moved"} workspace={null} onChanged={()=>{}}/>);
  fireEvent.click(screen.getByRole("tab",{name:"Missing & broken"}));
  fireEvent.click(screen.getByRole("button",{name:"Scan library files"}));
  await screen.findByRole("region",{name:"File scan results"});
  return b;
}
beforeEach(()=>localStorage.clear());

describe("Missing & broken: remove from library",()=>{
  it("offers selection only for files that are gone and never for a present one",async()=>{
    await scanned();
    expect(screen.queryByRole("checkbox",{name:"Select /Music/here.flac"})).toBeNull();
    fireEvent.click(screen.getByRole("checkbox",{name:"Show present files"}));
    expect(screen.queryByRole("checkbox",{name:"Select /Music/here.flac"})).toBeNull();
    expect(screen.getByRole("button",{name:/Remove from library/})).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox",{name:"Select all missing files"}));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("checkbox",{name:"Select /Music/drive.flac"})).not.toBeChecked();
  });
  it("previews the impact first and removes only after an explicit confirmation",async()=>{
    const b=await scanned();
    fireEvent.click(screen.getByRole("checkbox",{name:"Select all missing files"}));
    fireEvent.click(screen.getByRole("button",{name:/Remove from library/}));
    const preview=await screen.findByRole("region",{name:"Remove from library preview"});
    expect(b.repair.previewRemoval).toHaveBeenCalledWith(expect.any(String),"scan",["gone-a","gone-b"]);
    expect(b.repair.applyRemoval).not.toHaveBeenCalled();
    expect(within(preview).getByText(/Only the library records are removed/)).toBeInTheDocument();
    expect(within(preview).getByText(/2 playlist memberships in 1 playlist will be removed: Warm up \(2\)/)).toBeInTheDocument();
    expect(within(preview).getByText(/1 DJ-software sync link will be removed/)).toBeInTheDocument();
    expect(within(preview).getByText(/1 rekordbox link will be unlinked. rekordbox itself is not changed/)).toBeInTheDocument();
    expect(within(preview).getByText(/The cover art is kept until these saved records are dropped/)).toBeInTheDocument();
    const remove=within(preview).getByRole("button",{name:"Remove 2 records"});
    expect(remove).toBeDisabled();
    fireEvent.click(within(preview).getByRole("checkbox",{name:"I understand these library records will be removed"}));
    fireEvent.click(remove);
    await waitFor(()=>expect(b.repair.applyRemoval).toHaveBeenCalledWith(expect.any(String),"preview"));
    expect(await screen.findByText(/Removed 1 library record\. Undo is in History & recovery/)).toBeInTheDocument();
    expect(screen.queryByRole("region",{name:"File scan results"})).toBeNull();
  });
  it("asks about unplugged drives before removing records of files it could not reach",async()=>{
    const b=await scanned();
    fireEvent.click(screen.getByRole("button",{name:"Select unreachable (1)"}));
    fireEvent.click(screen.getByRole("button",{name:/Remove from library/}));
    const preview=await screen.findByRole("region",{name:"Remove from library preview"});
    expect(within(preview).getByRole("alert")).toHaveTextContent(/could not be reached.*reconnect it/);
    fireEvent.click(within(preview).getByRole("checkbox",{name:"I understand these library records will be removed"}));
    expect(within(preview).getByRole("button",{name:"Remove 1 record"})).toBeDisabled();
    fireEvent.click(within(preview).getByRole("checkbox",{name:"No drive is unplugged"}));
    expect(within(preview).getByRole("button",{name:"Remove 1 record"})).toBeEnabled();
    expect(b.repair.applyRemoval).not.toHaveBeenCalled();
  });
  it("lists tracks that will be kept and removes only the rest",async()=>{
    const b=bridges();
    vi.mocked(b.repair.previewRemoval).mockImplementation(async(_op,_scan,ids)=>({id:"preview",kind:"remove",scan_id:"scan",created:1,ready:1,impact,locations:[],files_checked:0,rows:[previewRow(ids[0]),previewRow(ids[1],{outcome:"blocked",reason:"Part of an open metadata draft; save or discard the draft first"})]}));
    await scanned(b);
    fireEvent.click(screen.getByRole("checkbox",{name:"Select all missing files"}));
    fireEvent.click(screen.getByRole("button",{name:/Remove from library/}));
    const preview=await screen.findByRole("region",{name:"Remove from library preview"});
    expect(within(preview).getByText("1 selected track will be kept")).toBeInTheDocument();
    expect(within(preview).getByText(/open metadata draft/)).toBeInTheDocument();
    expect(within(preview).getByRole("button",{name:"Remove 1 record"})).toBeInTheDocument();
  });
  it("blocks every action on a stale report",async()=>{
    const b=bridges();
    vi.mocked(b.api.scanBroken).mockResolvedValue({...scan,stale:true,stale_reason:"Source files changed; scan again"});
    await scanned(b);
    expect(await screen.findByText("Source files changed; scan again")).toBeInTheDocument();
    expect(screen.getByRole("checkbox",{name:"Select all missing files"})).toBeDisabled();
    expect(screen.getByRole("button",{name:/Remove from library/})).toBeDisabled();
    expect(screen.getByRole("button",{name:/Find moved files/})).toBeDisabled();
  });
  it("asks for archived tracks by default and can leave them out",async()=>{
    const b=bridges();
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"Missing & broken"}));
    fireEvent.click(screen.getByRole("button",{name:"Scan library files"}));
    await waitFor(()=>expect(b.api.scanBroken).toHaveBeenLastCalledWith(expect.any(String),{inspect_audio:false,include_archived:true}));
    fireEvent.click(screen.getByRole("checkbox",{name:"Include archived tracks"}));
    fireEvent.click(screen.getByRole("button",{name:"Scan library files"}));
    await waitFor(()=>expect(b.api.scanBroken).toHaveBeenLastCalledWith(expect.any(String),{inspect_audio:false,include_archived:false}));
  });
});

describe("Missing & broken: find moved files",()=>{
  it("previews matches by content and relinks only after confirmation",async()=>{
    const b=await scanned();
    fireEvent.click(screen.getByRole("checkbox",{name:"Select /Music/gone-a.flac"}));
    fireEvent.click(screen.getByRole("button",{name:/Find moved files/}));
    const panel=await screen.findByRole("region",{name:"Find moved files"});
    expect(within(panel).getByRole("button",{name:"Search for matching files"})).toBeDisabled();
    fireEvent.click(within(panel).getByRole("button",{name:"Choose search folder"}));
    await waitFor(()=>expect(within(panel).getByRole("textbox",{name:"Search location"})).toHaveValue("/Moved"));
    fireEvent.click(within(panel).getByRole("button",{name:"Search for matching files"}));
    expect(await within(panel).findByText("1 file found · 7 files checked")).toBeInTheDocument();
    expect(b.repair.previewRelink).toHaveBeenCalledWith(expect.any(String),"scan",["gone-a"],["/Moved"]);
    expect(within(panel).getByText("/Moved/gone-a.flac")).toBeInTheDocument();
    expect(b.repair.applyRelink).not.toHaveBeenCalled();
    fireEvent.click(within(panel).getByRole("button",{name:"Relink 1 track"}));
    await waitFor(()=>expect(b.repair.applyRelink).toHaveBeenCalledWith(expect.any(String),"preview"));
    expect(await screen.findByText(/Relinked 1 track to the files you moved\. Undo is in History & recovery/)).toBeInTheDocument();
  });
  it("shows why a track was not matched and offers nothing to relink",async()=>{
    const b=bridges();
    vi.mocked(b.repair.previewRelink).mockImplementation(async(_op,_scan,ids)=>({id:"preview",kind:"relink",scan_id:"scan",created:1,ready:0,impact,locations:["/Moved"],files_checked:3,rows:[previewRow(ids[0],{outcome:"not_found",reason:"A file with this name was found, but its content is different. It was not relinked"})]}));
    await scanned(b);
    fireEvent.click(screen.getByRole("checkbox",{name:"Select /Music/gone-a.flac"}));
    fireEvent.click(screen.getByRole("button",{name:/Find moved files/}));
    const panel=await screen.findByRole("region",{name:"Find moved files"});
    fireEvent.change(within(panel).getByRole("textbox",{name:"Search location"}),{target:{value:"/Moved"}});
    fireEvent.click(within(panel).getByRole("button",{name:"Search for matching files"}));
    expect(await within(panel).findByText(/content is different/)).toBeInTheDocument();
    expect(within(panel).getByRole("cell",{name:"Not found"})).toBeInTheDocument();
    expect(within(panel).getByRole("button",{name:"Relink 0 tracks"})).toBeDisabled();
  });
});

describe("Missing & broken: history and undo",()=>{
  it("lists repairs in History & recovery and undoes one with its saved ID",async()=>{
    const b=bridges();
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("remove"),undone_by:null},{entry:entry("relink",{id:"older"}),undone_by:"later"}]);
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    expect(await screen.findByText(/Removed from library ·/)).toBeInTheDocument();
    expect(screen.getByText(/Moved files found ·/)).toBeInTheDocument();
    expect(screen.getAllByRole("button",{name:/Undo (removal|relink)/})).toHaveLength(1);
    fireEvent.click(screen.getByRole("button",{name:"Undo removal"}));
    await waitFor(()=>expect(b.repair.undo).toHaveBeenCalledWith(expect.any(String),"entry"));
    expect(await screen.findByText(/Removal undone\. The records, playlist positions and metadata are back/)).toBeInTheDocument();
  });
});

describe("Missing & broken: undo notes",()=>{
  it("tells the DJ when a relinked file will be followed again by a watch folder",async()=>{
    const b=bridges();
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("relink"),undone_by:null}]);
    vi.mocked(b.repair.undo).mockResolvedValue(entry("relink",{undo_of:"entry",notes:["This file is still inside a watch folder, where it now is, so CueRithm will follow it again the next time that folder is scanned."]}));
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    fireEvent.click(await screen.findByRole("button",{name:"Undo relink"}));
    expect(await screen.findByText(/Relink undone\. The tracks point at their old locations again\. This file is still inside a watch folder/)).toBeInTheDocument();
  });
});

describe("Missing & broken: saved records",()=>{
  it("keeps saved records until they are explicitly discarded after an acknowledgement",async()=>{
    const b=bridges();
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("remove"),undone_by:null}]);
    vi.mocked(b.repair.discardUndo).mockResolvedValue({entry:entry("remove"),undone_by:null,undo_discarded:true});
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    fireEvent.click(await screen.findByRole("button",{name:"Discard saved records…"}));
    const discard=screen.getByRole("button",{name:"Discard saved records"});
    expect(discard).toBeDisabled();
    expect(b.repair.discardUndo).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("checkbox",{name:"I understand this removal can no longer be undone"}));
    fireEvent.click(discard);
    await waitFor(()=>expect(b.repair.discardUndo).toHaveBeenCalledWith(expect.any(String),"entry"));
    expect(await screen.findByText(/Saved records discarded\. The history line stays/)).toBeInTheDocument();
  });
  it("offers no Undo for a removal whose saved records were discarded, and keeps no track names for it",async()=>{
    const b=bridges();
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("remove",{tracks:[],track_count:3}),undone_by:null,undo_discarded:true}]);
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    expect(await screen.findByText(/this removal can no longer be undone\./)).toBeInTheDocument();
    expect(screen.queryByRole("button",{name:"Undo removal"})).toBeNull();
    expect(screen.queryByRole("button",{name:"Discard saved records…"})).toBeNull();
    expect(screen.getByText(/3 tracks/)).toBeInTheDocument();
    expect(screen.queryByText("Affected tracks")).toBeNull();
  });
});

describe("Missing & broken: table layout",()=>{
  it("keeps outcome labels whole instead of breaking inside a word",async()=>{
    await scanned();
    const cell=screen.getByRole("cell",{name:"Unavailable"});
    expect(cell).toHaveClass("maintenance-nowrap");
    expect(screen.getByRole("columnheader",{name:"Outcome"})).toHaveClass("maintenance-nowrap");
    // jsdom has no layout and Vitest drops stylesheet text, so the class is the testable contract;
    // styles.css gives it `white-space:nowrap; overflow-wrap:normal`.
  });
});

describe("History & recovery: both kinds of removal",()=>{
  const merge={id:"merge-1",preview_id:"merge-preview",stamp,archived_ids:[] as string[],restored_ids:[] as string[],affected_playlists:[] as string[],undo_of:null,updated_ids:[] as string[],removed_ids:["b"],trashed:[],permanently_deleted:[],not_deleted:[],links_not_restored:0};
  it("shows duplicate merges and Missing & broken repairs under one Undo limit, and each Undo goes to its own engine",async()=>{
    const b=bridges();
    const undoMerge=vi.fn(async()=>({...merge,id:"undo-1",undo_of:"merge-1",removed_ids:[] as string[],restored_ids:["b"]}));
    Object.assign(b.api,{undoMerge});
    vi.mocked(b.api.mergeHistory).mockResolvedValue([{result:merge,created:2,undone_by:null,undo_expired:false,undo_limit:10}]);
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("remove"),undone_by:null,undo_discarded:false,undo_limit:10}]);
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    const history=await screen.findByRole("region",{name:"Maintenance history"});
    await within(history).findByRole("button",{name:"Undo merge"});
    // The same sentence, with the same number, wherever a removal can be undone.
    expect(within(history).getAllByText(/Undo is kept for your last 10 removals of library records; duplicate merges that delete files and Missing & broken removals count together\. After that a removal is final\./)).toHaveLength(2);
    fireEvent.click(within(history).getByRole("button",{name:"Undo merge"}));
    await waitFor(()=>expect(undoMerge).toHaveBeenCalledWith(expect.any(String),"merge-1"));
    expect(b.repair.undo).not.toHaveBeenCalled();
    fireEvent.click(await within(history).findByRole("button",{name:"Undo removal"}));
    await waitFor(()=>expect(b.repair.undo).toHaveBeenCalledWith(expect.any(String),"entry"));
    expect(undoMerge).toHaveBeenCalledTimes(1);
  });
  it("says a removal pushed out of the shared window is final, whichever kind it is",async()=>{
    const b=bridges();
    vi.mocked(b.api.mergeHistory).mockResolvedValue([{result:merge,created:2,undone_by:null,undo_expired:true,undo_limit:10}]);
    vi.mocked(b.repair.history).mockResolvedValue([{entry:entry("remove",{tracks:[]}),undone_by:null,undo_discarded:true,undo_limit:10}]);
    render(<Maintenance api={b.api} repair={b.repair} chooseFolder={async()=>null} workspace={null} onChanged={()=>{}}/>);
    fireEvent.click(screen.getByRole("tab",{name:"History & recovery"}));
    const history=await screen.findByRole("region",{name:"Maintenance history"});
    expect(await within(history).findByText(/The removal is final\. Undo is no longer available: the removed library records were kept only for the last 10 removals/)).toBeInTheDocument();
    expect(within(history).getByText(/this removal can no longer be undone\./)).toBeInTheDocument();
    expect(within(history).queryByRole("button",{name:"Undo merge"})).toBeNull();
    expect(within(history).queryByRole("button",{name:"Undo removal"})).toBeNull();
  });
});
