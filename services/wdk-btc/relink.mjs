// Self-healing Electrum link for the WDK treasury service.
//
// wdk-wallet-btc's MempoolElectrumClient gives up after its retry budget and then every call
// fails with "Connection to server lost, please retry" until the process restarts. `withLink`
// wraps a wallet call: on a dropped link it reconnects once (shared between concurrent callers)
// and retries the call; any other error passes through untouched.
export const LINK_DOWN = /connection to server lost|socket|ECONN|closed|timed? ?out|not connected/i

/**
 * @param {{ reconnect: () => Promise<void>, log?: (level: string, msg: string, extra?: object) => void }} client
 * @returns {<T>(fn: () => Promise<T>) => Promise<T>}
 */
export function withLink (client) {
  const log = client.log || (() => {})
  let reconnecting = null
  const relink = (reason) => {
    if (!reconnecting) {
      log('warn', 'electrum link down; reconnecting', { reason: String(reason).slice(0, 120) })
      reconnecting = Promise.resolve()
        .then(() => client.reconnect())
        .then(() => log('info', 'electrum link restored'))
        .finally(() => { reconnecting = null })
    }
    return reconnecting
  }
  return async function linked (fn) {
    try { return await fn() } catch (err) {
      if (!LINK_DOWN.test(String(err?.message || err))) throw err
      await relink(err?.message)
      return fn()
    }
  }
}
