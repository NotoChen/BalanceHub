import type {
  AgentAssetAccessRisk,
  AgentAssetOperationOutcome,
  AgentAssetOperationPhase,
  AgentAssetScope,
} from "../stores/provider-types";

/**
 * 资产词表只依赖类型，不依赖组合式函数状态。放在 utils 里，启动期就会
 * 用到的后台任务列表、目录展示等模块不必为了几个标签加载整套资产控制台。
 */
export const agentAssetScopeLabels: Record<AgentAssetScope, string> = {
  user: "用户", workspace: "工作区", local: "本机", system: "系统", managed: "托管",
};
export const agentAssetOperationPhaseLabels: Record<AgentAssetOperationPhase, string> = {
  preparing: "准备中", waitingForLock: "等待同源操作", revalidating: "重新校验", applying: "正在应用", verifying: "验证结果", completed: "已完成",
};
export const agentAssetOperationOutcomeLabels: Record<AgentAssetOperationOutcome, string> = {
  canceledBeforeCommit: "提交前已取消", unchangedConflict: "配置冲突，未修改", unchangedFailure: "操作失败，未修改",
  appliedVerified: "已应用并验证", appliedUnverified: "已应用，尚未验证", outcomeUnknown: "结果未确认",
};
export const agentAssetAccessRiskLabels: Record<AgentAssetAccessRisk, string> = {
  externalPathnameRace: "系统应用会按路径打开文件；文件路径在交给外部应用后仍可能被其他进程替换。",
  rawSensitiveContent: "外部应用将读取文件原始内容，其中可能包含密钥、令牌、Cookie 或密码。",
};
