import {invoke} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import type {UnusedRemovalPreview,UnusedRemovalResult,UnusedRemovalHistory,DuplicateRecovery,DuplicateMergeResult,DuplicateMergePreview,DuplicateMergeHistory,DuplicateScanRequest,DuplicateScanReport,DuplicateReview,DuplicateReviewEdit,DuplicateIgnores,MaintenanceStamp,UnusedScanRequest,UnusedScanReport,BrokenScanRequest,BrokenScanReport,MaintenanceProgress,MaintenanceChange,MergeDisposition} from "./maintenance-types";

// Give every long operation a fresh ID (crypto.randomUUID()) and retain it for cancellation.
// A cancelled operation keeps the slot until its original promise settles.
export const nativeMaintenance={
  previewUnusedRemoval:(operationId:string,scanId:string,selected:string[])=>invoke<UnusedRemovalPreview>("maintenance_preview_unused_removal",{operationId,scanId,selected}),
  applyUnusedRemoval:(operationId:string,id:string)=>invoke<UnusedRemovalResult>("maintenance_apply_unused_removal",{operationId,id}),
  unusedRemovalHistory:()=>invoke<UnusedRemovalHistory[]>("maintenance_unused_removal_history"),
  retryUnusedRecovery:(operationId:string,id:string)=>invoke<UnusedRemovalHistory[]>("maintenance_retry_unused_recovery",{operationId,id}),
  applyMerge:(operationId:string,id:string,confirmDeletion=false)=>invoke<DuplicateMergeResult>("maintenance_apply_merge",{operationId,id,confirmDeletion}),
  undoMerge:(operationId:string,id:string)=>invoke<DuplicateMergeResult>("maintenance_undo_merge",{operationId,id}),
  recovery:()=>invoke<DuplicateRecovery[]>("maintenance_recovery"),
  retryRecovery:(operationId:string,id:string)=>invoke<DuplicateRecovery[]>("maintenance_retry_recovery",{operationId,id}),
  previewMerge:(operationId:string,reviewId:string,expectedRevision:number,groups:string[],disposition:MergeDisposition)=>invoke<DuplicateMergePreview>("maintenance_preview_merge",{operationId,reviewId,expectedRevision,groups,disposition}),
  mergeHistory:()=>invoke<DuplicateMergeHistory[]>("maintenance_merge_history"),
  scanDuplicates:(operationId:string,request:DuplicateScanRequest)=>invoke<DuplicateScanReport>("maintenance_scan_duplicates",{operationId,request}),
  duplicateScan:(operationId:string,id:string)=>invoke<DuplicateScanReport>("maintenance_duplicate_scan",{operationId,id}),
  beginReview:(operationId:string,id:string)=>invoke<DuplicateReview>("maintenance_begin_review",{operationId,id}),
  getReview:(operationId:string,id:string)=>invoke<DuplicateReview>("maintenance_get_review",{operationId,id}),
  editReview:(operationId:string,id:string,expectedRevision:number,edit:DuplicateReviewEdit)=>invoke<DuplicateReview>("maintenance_edit_review",{operationId,id,expectedRevision,edit}),
  ignores:()=>invoke<DuplicateIgnores>("maintenance_ignores"),
  setIgnores:(trackIds:string[],ignored:boolean,expected:MaintenanceStamp)=>invoke<DuplicateIgnores>("maintenance_set_ignores",{trackIds,ignored,expected}),
  scanUnused:(operationId:string,request:UnusedScanRequest)=>invoke<UnusedScanReport>("maintenance_scan_unused",{operationId,request}),
  unusedScan:(operationId:string,id:string)=>invoke<UnusedScanReport>("maintenance_unused_scan",{operationId,id}),
  exportUnused:(operationId:string,id:string)=>invoke<string>("maintenance_export_unused",{operationId,id}),
  scanBroken:(operationId:string,request:BrokenScanRequest)=>invoke<BrokenScanReport>("maintenance_scan_broken",{operationId,request}),
  brokenScan:(operationId:string,id:string)=>invoke<BrokenScanReport>("maintenance_broken_scan",{operationId,id}),
  cancel:(operationId:string)=>invoke<boolean>("maintenance_cancel",{operationId}),
  onProgress:(handler:(event:MaintenanceProgress)=>void)=>listen<MaintenanceProgress>("maintenance-progress",event=>handler(event.payload)),
  onChange:(handler:(event:MaintenanceChange)=>void)=>listen<MaintenanceChange>("maintenance-change",event=>handler(event.payload)),
};
export type MaintenanceBridge=typeof nativeMaintenance;
