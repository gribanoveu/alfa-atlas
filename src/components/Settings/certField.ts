/** Format hint shown in every «Доверенный сертификат» box — Jira, LLM
 * providers and embeddings all take the same PEM bundle.
 *
 * It is a placeholder rather than a prefilled value on purpose: none of
 * these fields ever renders the certificate the build supplies. Showing it
 * would make a build default indistinguishable from the user's own, and
 * saving the box would pin that default as an override, after which a
 * manifest update would stop reaching the user. Whether a build certificate
 * exists is said in the hint under the field instead. */
export const CERT_PLACEHOLDER = "-----BEGIN CERTIFICATE-----";
