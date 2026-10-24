import type {SmartStamp} from "./playlist-types";

export interface MaintenanceStamp {inputs:SmartStamp;maintenance_revision:number}
export type MaintenanceFileState="present"|"missing"|"unavailable"|"symlink"|"not_regular"|"changed"|"video"|"no_audio"|"decode_error"|"unsupported"|"timed_out"|"duration_skipped";
export interface AudioFingerprint {source_sha256:string;decoder_identity:string;algorithm:string;digest:string;sample_rate:number;channels:number;audio_stream_index:number;audio_stream_count:number;pcm_format:string}
export interface DuplicateScanRequest {scope:"full_library"|"selected_playlists"|"full_library_filtered";playlist_ids:string[];tolerance:"None"|"Low"|"Medium"|"High"|"High_IgnoreParentheses";ignored_paths:string[]}
export interface DuplicateScanTrack {id:string;display_path:string;title:string;artist:string;archived:boolean;state:MaintenanceFileState;detail:string|null;observed_sha256:string|null;source_changed:boolean;fingerprint:AudioFingerprint|null;cached:boolean}
export interface DuplicateScanGroup {id:string;method:"fingerprint"|"artist_title";track_ids:string[];suggested_keeper:string|null;suggested_tags_from:string|null}
interface Report {id:string;created:number;stamp:MaintenanceStamp;stale:boolean;stale_reason:string|null}
export interface DuplicateSkippedTrack {id:string;state:MaintenanceFileState}
export interface DuplicateScanReport extends Report {request:DuplicateScanRequest;tracks:DuplicateScanTrack[];groups:DuplicateScanGroup[];skipped_unavailable:DuplicateSkippedTrack[];cache_hits:number;fingerprints_computed:number}
export interface ReviewCandidate {id:string;artist:string;title:string;duration:number;location:string;comparison_path:string;bitrate:number|null;cue_count:number;present:boolean;ignored:boolean}
export interface ReviewTrack {candidate:ReviewCandidate;extra1:string}
export type ReviewState="unlocked"|"locked"|"copy_cues";
export interface ReviewGroup {id:string;exact_audio:boolean;tracks:ReviewTrack[];keeper:string|null;tags_from:string|null;state:ReviewState;ignored:string[];changed_selection:boolean}
export interface DuplicateReview extends Report {scan_id:string;revision:number;groups:ReviewGroup[]}
export type GroupEdit={kind:"select";track_id:string}|{kind:"use_tags";track_id:string|null}|{kind:"lock"}|{kind:"unlock";force:boolean}|{kind:"toggle_ignore";track_id:string};
export type Preference={kind:"folder";path:string}|{kind:"most_cuepoints"}|{kind:"highest_bitrate"}|{kind:"title_contains"|"extra1_contains"|"extra1_not_contains";value:string};
export type DuplicateReviewEdit={kind:"group";group_id:string;edit:GroupEdit}|{kind:"prefer";preference:Preference;override_locked:boolean;use_best_cues:boolean};
export interface DuplicateIgnores {stamp:MaintenanceStamp;tracks:{id:string;display_path:string;title:string;artist:string}[]}
export interface UnusedScanRequest {root:string;extension_mode:"all"|"include"|"exclude";extensions:string[]}
export interface UnusedFile {id:string;display_path:string;size_bytes:number;source_sha256:string}
export interface UnusedScanReport extends Report {request:UnusedScanRequest;files:UnusedFile[];size_bytes:number;complete:boolean;issues:{display_path:string;message:string}[]}
export interface BrokenScanRequest {inspect_audio:boolean;include_archived?:boolean}
export interface BrokenScanReport extends Report {request:BrokenScanRequest;decoder_identity:string|null;tracks:{id:string;archived?:boolean;display_path:string;state:MaintenanceFileState;detail:string|null;observed_sha256:string|null;source_changed:boolean}[]}
export type MaintenanceProgress={operation_id:string;kind:"duplicates";progress:{completed:number;total:number;track_id:string|null;cached:boolean}}|{operation_id:string;kind:"unused"|"broken";progress:{completed:number;total:number|null;current_path:string|null}};
export interface MaintenanceChange {review_id:string|null;stamp:MaintenanceStamp}

export type MergeDisposition="archive"|"delete";
export interface GroupMerge {group_id:string;mode:ReviewState;exact_audio:boolean;keeper:string;archive_ids:string[];delete_ids:string[];unarchive_keeper:boolean;copies:{from:string;to:string}[]}
export interface DuplicatePlaylistChange {id:string;before:string[];after:string[]}
export interface DuplicateDeletionFile {track_id:string;display_path:string;size_bytes:number;keeper_id:string;keeper_display_path:string;identity:"same_file"|"same_audio";sha256:string;keeper_sha256?:string;permanent:boolean}
export type DuplicateSkipReason="not_verified_identical"|"file_changed"|"file_unavailable"|"shared_with_remaining_record"|"same_file_as_keeper"|"busy"|"trash_failed";
export interface DuplicateDeletionSkip {track_id:string;display_path:string;keeper_id:string|null;reason:DuplicateSkipReason;detail:string|null}
export interface DuplicateRemovalImpact {track_id:string;cue_revisions:number;tags:number;sync_links:number;other_records:number}
export interface DuplicateDeletionPlan {files:DuplicateDeletionFile[];skipped:DuplicateDeletionSkip[];impact:DuplicateRemovalImpact[];undo_limit:number}
export interface DuplicateMergePreview {id:string;review_id:string;review_revision:number;created:number;stamp:MaintenanceStamp;plan:{groups:GroupMerge[];disposition:MergeDisposition};playlists:DuplicatePlaylistChange[];deletion:DuplicateDeletionPlan|null}
export interface DuplicateMergeResult {id:string;preview_id:string;stamp:MaintenanceStamp;archived_ids:string[];restored_ids:string[];affected_playlists:string[];undo_of:string|null;updated_ids:string[];removed_ids:string[];trashed:DuplicateDeletionFile[];permanently_deleted:DuplicateDeletionFile[];not_deleted:DuplicateDeletionSkip[];links_not_restored:number}
export interface DuplicateMergeHistory {result:DuplicateMergeResult;created:number;undone_by:string|null;undo_expired:boolean;undo_limit:number}

export interface DuplicateRecovery {id:string;kind:string;state:string;created:number;error:string|null;files_moved?:boolean}

export interface UnusedRemovalFile extends UnusedFile {role:"selected"|"sibling"|"folder_metadata";permanent?:boolean}
export type UnusedRemovalDisposal="trashed"|"deleted"|"left_in_place"|"gone";
export interface UnusedRemovalOutcome {id:string;display_path:string;disposal:UnusedRemovalDisposal;reason:string|null}
export interface UnusedRemovalPreview {directories:string[];id:string;scan_id:string;stamp:MaintenanceStamp;files:UnusedRemovalFile[];size_bytes:number}
export interface UnusedRemovalResult {directories:string[];id:string;preview_id:string;stamp:MaintenanceStamp;files:UnusedRemovalFile[];size_bytes:number;restored:boolean;undo_of:string|null;outcomes?:UnusedRemovalOutcome[]}
export interface UnusedRemovalHistory {result:UnusedRemovalResult;state:string;error:string|null;undone_by:string|null;cleanup_pending:boolean}
