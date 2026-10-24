import type {NativePath} from "./native-paths";
import type {TargetEnvironment,SyncConfigurationBinding} from "./sync-target-types";
export type DjSoftware="virtual_dj"|"engine_dj"|"djay_pro"|"serato4"|"pioneer_usb"|"mixxx";
export interface SyncTarget {software:DjSoftware;path:NativePath;source_root:NativePath|null;environment?:TargetEnvironment}
export interface DiscoveredTarget {software:DjSoftware;path:string}
export type SyncTextField="title"|"artist"|"album"|"genre"|"label"|"comment"|"key";
/** Mixxx only: update the library or replace it whole, what happens to tracks already in Mixxx, and what playlists become. */
export interface MixxxOptions {library:"update"|"overwrite";existing_tracks:"keep"|"replace";container:"playlists"|"crates";replace_metadata:boolean;replace_key:boolean;replace_cues:boolean}
export interface SyncOptions {
  engine_copy_audio:boolean;
  materialize_smartlists:boolean;
  key_notation:"original"|"musical"|"camelot"|"open_key";
  field_mappings:Partial<Record<SyncTextField,SyncTextField>>;
  usb_cue_destination:"hot"|"memory"|"both";
  usb_cdj_compatibility:"off"|"detect"|"auto_fix";
  usb_cleanup_unused:boolean;
  skip_beatgrids:boolean;
  nearest_color:boolean;
  beatgrid_lock:"default"|"lock"|"unlock";
  add_album_art:boolean;
  extra_cue_markers:boolean;
  mixxx:MixxxOptions;
}
export interface DjSyncRequest {target:SyncTarget;scope:{type:"full"}|{type:"playlist";id:string}|{type:"changed"};options:SyncOptions;configuration?:SyncConfigurationBinding|null}
export interface FileChange {target:NativePath;before_sha256:string|null;after_sha256:string|null;bytes:number}
export interface DirectoryChange {target:NativePath;before_manifest:unknown;after_exists:boolean}
export interface CaseRenameChange {from:NativePath;to:NativePath}
export interface SyncPreview {renames?:CaseRenameChange[];deleted_playlists:number;retained_external_playlists:number;id:string;target:SyncTarget;tracks:number;playlists:number;bytes_to_write:number;files:FileChange[];warnings:string[];directories:DirectoryChange[]}
export type JournalState="prepared"|"applying"|"applied"|"restoring"|"restored";
export interface FileJournal {renames?:CaseRenameChange[];id:string;state:JournalState;changes:FileChange[];directories:DirectoryChange[]}
export interface DjSyncHistoryEntry {needs_acknowledgement:boolean;needs_restore_acknowledgement:boolean;id:string;state:JournalState|null;target:SyncTarget|null;tracks:number;playlists:number;created:number|null;error:string|null;restore_expired?:boolean}
export const syncTargets:{id:DjSoftware;name:string;location:string}[]=[
  {id:"virtual_dj",name:"VirtualDJ",location:"VirtualDJ library folder"},
  {id:"engine_dj",name:"Engine DJ",location:"Engine Library folder or target drive"},
  {id:"djay_pro",name:"djay Pro",location:"djay Media Library folder"},
  {id:"serato4",name:"Serato DJ Pro 4",location:"Serato Library folder"},
  {id:"pioneer_usb",name:"Pioneer USB",location:"Target USB drive"},
  {id:"mixxx",name:"Mixxx",location:"Mixxx settings folder"},
];
export const defaultSyncOptions:SyncOptions={materialize_smartlists:false,engine_copy_audio:false,key_notation:"original",field_mappings:{},usb_cue_destination:"hot",usb_cdj_compatibility:"off",usb_cleanup_unused:false,skip_beatgrids:false,nearest_color:true,beatgrid_lock:"default",add_album_art:false,extra_cue_markers:false,mixxx:{library:"update",existing_tracks:"keep",container:"playlists",replace_metadata:true,replace_key:true,replace_cues:true}};
