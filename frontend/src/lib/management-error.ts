export class ManagementAuthError extends Error {
  constructor(message = 'Management authorization failed') {
    super(message)
    this.name = 'ManagementAuthError'
  }
}
