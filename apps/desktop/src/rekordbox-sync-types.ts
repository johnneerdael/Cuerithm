import type {SyncConfigurationBridge,SyncConfigurationBinding,TargetEnvironment} from "./sync-target-types";
export type NativeFieldDecision="use_local"|"use_remote"|"clear_local"|"clear_remote"|"accept_equal"|"skip";
export type NativeFieldObservation={state:"unavailable"}|{state:"present";value:unknown};
export type NativeReconciliation={state:"unchanged"}|{state:"proposed";decision:NativeFieldDecision}|{state:"conflict";reason:"observation_unavailable"|"initial_acknowledgement"|"concurrent_changes"|"ownership"|"unowned";suggestion:NativeFieldDecision|null};
export interface RekordboxSyncTarget {id:string;installation:{root:string;executable:string;version:string;major:number;version_source:string};database:string;options_file:string;asset_root:string;environment?:TargetEnvironment}
export interface RekordboxManualTarget {executable:string;version_hint:string|null;database:string;options_file:string;asset_root:string;environment:TargetEnvironment}
export interface RekordboxSyncPreflight {target:RekordboxSyncTarget;readable:boolean;writable:boolean;restrictions:string[];schema_fingerprint:string|null;identity:unknown}
export interface RekordboxSyncOptions {metadata_fields:string[];cues:boolean;beat_grid:boolean;artwork:boolean;playlists:boolean;my_tags:boolean;materialize_smart_playlists:boolean;convert_incompatible_audio:boolean;nearest_color:boolean}
export type RekordboxSyncScope={scope:"full"}|{scope:"changed"}|{scope:"playlist";playlist_id:string};
export type RekordboxDecisions=Record<string,Record<string,NativeFieldDecision>>;
export interface RekordboxSyncRequest {target_id:string;scope:RekordboxSyncScope;options:RekordboxSyncOptions;decisions:RekordboxDecisions;configuration?:SyncConfigurationBinding|null}
export interface RekordboxSyncFieldReview {track_id:string;title:string;new_track?:boolean;field:string;local:NativeFieldObservation;remote:NativeFieldObservation;result:NativeReconciliation;decision:NativeFieldDecision|null}
export type RekordboxEntityKind="playlist"|"folder"|"category"|"tag";
export interface RekordboxEntityMember {title:string;position:number|null;external:boolean}
export interface RekordboxEntityState {name:string;parent_path:string[];representation:"folder"|"ordinary"|"native_smart"|"category"|"tag";position:number;member_count:number;members:RekordboxEntityMember[];external_member_count:number}
export interface RekordboxEntityChange {local_id:string;kind:RekordboxEntityKind;action:"create"|"update"|"remove";ownership:"new"|"owned"|"matched";materialized_smart:boolean;before:RekordboxEntityState|null;after:RekordboxEntityState|null}
export interface RekordboxPendingEntity {kind:RekordboxEntityKind;name:string;unresolved_members:number;member_titles:string[]}
export interface RekordboxSyncPreview {id:string;target_id:string;library_revision:number;fields:RekordboxSyncFieldReview[];entity_changes:RekordboxEntityChange[];pending_entities:RekordboxPendingEntity[];tracks:number;changed_rows:number;skipped_fields:number;warnings:string[];can_apply:boolean;media_copies:number;copied_audio_bytes:number}
export interface RekordboxSyncOutcome {id:string;status:string;tracks:number;changed_rows:number;acknowledged_fields:number;skipped_fields:number}
export interface RekordboxSyncHistoryEntry {id:string;target_id:string;status:string;created:number;updated:number;outcome:RekordboxSyncOutcome|null;restore_available?:boolean}
export interface RekordboxBridge extends SyncConfigurationBridge {
 configure:(request:RekordboxManualTarget)=>Promise<RekordboxSyncTarget>;
 discover:()=>Promise<RekordboxSyncTarget[]>;
 preflight:(operationId:string,targetId:string,configuration?:SyncConfigurationBinding)=>Promise<RekordboxSyncPreflight>;
 preview:(operationId:string,request:RekordboxSyncRequest)=>Promise<RekordboxSyncPreview>;
 apply:(operationId:string,id:string)=>Promise<RekordboxSyncOutcome>;
 restore:(operationId:string,id:string)=>Promise<RekordboxSyncOutcome>;
 recover:(operationId:string,id:string)=>Promise<RekordboxSyncOutcome>;
 discard:(id:string)=>Promise<void>;
 history:()=>Promise<RekordboxSyncHistoryEntry[]>;
 cancel:(operationId:string)=>Promise<boolean>;
}
export const rekordboxMetadataFields=["title","artist","album","album_artist","genre","label","comment","remixer","composer","lyricist","mix_name","key","bpm","rating","release_year","release_date","track_number","disc_number"];
export const defaultRekordboxOptions:RekordboxSyncOptions={metadata_fields:rekordboxMetadataFields,cues:true,beat_grid:true,artwork:true,playlists:true,my_tags:true,materialize_smart_playlists:false,convert_incompatible_audio:false,nearest_color:false};
