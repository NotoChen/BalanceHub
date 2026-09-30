import { Channel, invoke } from '@tauri-apps/api/core'
import type {
  AgentAssetCatalog, AgentCatalogDefinition, AgentCatalogSaveRequest, AgentCatalogDeleteRequest,
  AgentCatalogAdoptRequest, AgentCatalogPlanRequest, AgentCatalogAgentPanelRequest, AgentCatalogAgentPanel,
  AgentCatalogPlan, AgentCatalogApplyRequest, AgentCatalogOperation,
  AgentCatalogRelationPreviewRequest, AgentCatalogRelationPreview, AgentCatalogRelationCommitRequest, AgentCatalogRelationCommitResult,
  AgentCatalogContent, AgentCatalogContentRequest, AgentCatalogMcpInput, AgentMcpFormRead,
} from '../stores/agent-catalog-types'
import type { AgentCliKind } from '../stores/provider-types'
import type { AgentConfigurationFormat } from '../stores/agent-configuration-types'

export const readAgentMcpForm = (text: string, format: AgentConfigurationFormat, agentKind: AgentCliKind | null) =>
  invoke<AgentMcpFormRead>('read_agent_mcp_form', { text, format, agentKind })
export const renderAgentMcpForm = (input: AgentCatalogMcpInput, originalText: string, format: AgentConfigurationFormat, agentKind: AgentCliKind | null) =>
  invoke<string>('render_agent_mcp_form', { input, originalText, format, agentKind })

export const getAgentAssetCatalog = (workspace: string | null = null, onSnapshot?: (catalog: AgentAssetCatalog) => void) => {
  const progress = new Channel<AgentAssetCatalog>()
  if (onSnapshot) progress.onmessage = onSnapshot
  return invoke<AgentAssetCatalog>('get_agent_asset_catalog', { workspace, progress })
}
export const hasAgentCatalogChanges = (revision: string, workspace: string | null = null) =>
  invoke<boolean>('has_agent_catalog_changes', { workspace, revision })
export const getAgentCatalogRevision = (workspace: string | null = null) =>
  invoke<string | null>('get_agent_catalog_revision', { workspace })
export const readAgentCatalogContent = (request: AgentCatalogContentRequest, onProgress: (content: AgentCatalogContent) => void) => {
  const progress = new Channel<AgentCatalogContent>()
  progress.onmessage = onProgress
  return invoke<AgentCatalogContent>('read_agent_catalog_content', { request, progress })
}
export const cancelAgentCatalogRead = (requestId: string) =>
  invoke<void>('cancel_agent_catalog_read', { requestId })
export const getAgentCatalogDefinition = (assetId: string) =>
  invoke<AgentCatalogDefinition>('get_agent_catalog_definition', { assetId })
export const saveAgentCatalogDefinition = (request: AgentCatalogSaveRequest) =>
  invoke<AgentCatalogDefinition>('save_agent_catalog_definition', { request })
export const deleteAgentCatalogDefinition = (request: AgentCatalogDeleteRequest) =>
  invoke<void>('delete_agent_catalog_definition', { request })
export const adoptAgentCatalogAsset = (request: AgentCatalogAdoptRequest) =>
  invoke<AgentCatalogDefinition>('adopt_agent_catalog_asset', { request })
export const getAgentCatalogAgentPanel = (request: AgentCatalogAgentPanelRequest, requestId: string) =>
  invoke<AgentCatalogAgentPanel>('get_agent_catalog_agent_panel', { request, requestId })
export const previewAgentCatalogRelation = (request: AgentCatalogRelationPreviewRequest) =>
  invoke<AgentCatalogRelationPreview>('preview_agent_catalog_relation', { request })
export const commitAgentCatalogRelation = (request: AgentCatalogRelationCommitRequest) =>
  invoke<AgentCatalogRelationCommitResult>('commit_agent_catalog_relation', { request })
export const planAgentCatalog = (request: AgentCatalogPlanRequest, requestId: string) =>
  invoke<AgentCatalogPlan>('plan_agent_catalog', { request, requestId })
export const applyAgentCatalog = (request: AgentCatalogApplyRequest) =>
  invoke<AgentCatalogOperation>('apply_agent_catalog', { request })
export const getAgentCatalogOperation = (operationId: string) =>
  invoke<AgentCatalogOperation>('get_agent_catalog_operation', { operationId })
export const listAgentCatalogOperations = () =>
  invoke<AgentCatalogOperation[]>('list_agent_catalog_operations')
export const cancelAgentCatalogOperation = (operationId: string) =>
  invoke<AgentCatalogOperation>('cancel_agent_catalog_operation', { operationId })
