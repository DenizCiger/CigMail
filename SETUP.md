# Setting up CigMail

CigMail talks to Gmail and Outlook through their official APIs. Each needs an OAuth app that **you** own. Both are free, take about 5 minutes, and stay private to you. IMAP accounts and the ChatGPT connection need no setup.

You can enter the IDs in **Settings → Connections**, or bake them in at build time:

```bash
export CIGMAIL_GOOGLE_CLIENT_ID=...apps.googleusercontent.com
export CIGMAIL_GOOGLE_CLIENT_SECRET=...
export CIGMAIL_MICROSOFT_CLIENT_ID=00000000-0000-0000-0000-000000000000
```

## Gmail (Google Cloud)

1. Open <https://console.cloud.google.com/>, then select your project (or create one called *CigMail*).
2. **APIs & Services → Library** → enable the **Gmail API**.
3. **APIs & Services → OAuth consent screen** (now called *Google Auth Platform*):
   - User type: **External**.
   - App name: *CigMail*. Use your own email for the support and developer contact.
   - **Data access / Scopes:** add `.../auth/gmail.modify`, `openid`, `.../auth/userinfo.email` and `.../auth/userinfo.profile`.
   - **Audience:** add your Gmail address(es) as **test users**.
4. **Credentials → Create credentials → OAuth client ID → Application type: Desktop app**. Copy the **Client ID** and **Client secret** into CigMail.
5. Optional but recommended: in *Audience*, click **Publish app** ("In production"). In *Testing* mode Google expires refresh tokens after 7 days, so you'd have to sign in again every week. An unverified personal app works fine. Google just shows a "Google hasn't verified this app" screen. Click *Advanced → Go to CigMail*.

## Outlook / Microsoft 365 (Entra ID)

1. Open <https://entra.microsoft.com/> → **App registrations → New registration**. A personal Microsoft account works for this; you don't need the school tenant.
   - Name: *CigMail*
   - Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**
   - Redirect URI: platform **Public client/native (mobile & desktop)**, URI `http://localhost`
2. **Authentication** → *Advanced settings* → **Allow public client flows: Yes**.
3. **API permissions → Add a permission → Microsoft Graph → Delegated**: `User.Read`, `Mail.ReadWrite`, `Mail.Send`, `offline_access`, `openid`, `email`, `profile`.
4. For the school IMAP fallback (below), also add **APIs my organization uses → Office 365 Exchange Online → Delegated**: `IMAP.AccessAsUser.All` and `SMTP.Send`.
5. Copy the **Application (client) ID** into CigMail. Leave the tenant as `common`.

### School account blocked?

Some universities only allow admin-approved apps. If you see *"Need admin approval"*:

1. Try **Add account → IMAP / SMTP → Preset "Microsoft 365 (OAuth)"**. This uses IMAP with Microsoft sign-in, and some tenants allow it when Graph is blocked.
2. If that's blocked too, ask IT to approve the app (send them the client ID), or check whether they allow app passwords.
3. As a last resort, forward school mail to another account.

## ChatGPT (AI features)

**Settings → AI → Connect** opens the ChatGPT sign-in in your browser. This is the same flow the Codex CLI uses, through the port `localhost:1455`. Close any running `codex login` first. Usage counts toward your ChatGPT plan's limits.

> This connection is unofficial: it's how [openai-oauth](https://github.com/EvanZhouDev/openai-oauth) and the Codex CLI work. OpenAI may change or block it. The AI code sits behind one function (`Ai::stream` in `src-tauri/src/ai.rs`), so you can swap in an API key or another provider if needed.

## Where things are stored

- Mail cache and settings: `~/.local/share/dev.ciger.cigmail/cigmail.db` (SQLite)
- Tokens and passwords: the OS keyring (Secret Service on Linux). If no keyring daemon is running, they fall back to the database.
