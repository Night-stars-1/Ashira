/** The approval modes the chat switch and 设置 → 通用 offer. */
export const approvalOptions = [
  { value: 'manual', label: '手动审批', detail: '危险 Shell 命令和未授权的 MCP 工具需询问。' },
  { value: 'auto', label: '自动审批', detail: '允许低风险、受信任或白名单操作，自动拒绝危险操作和未授权的 MCP 工具。' },
  { value: 'full', label: '完整权限', detail: '跳过工具审批，但不会增加 Ashira 的系统权限。' },
] as const
