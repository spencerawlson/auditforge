//! Register the completed LabNet engagements into an existing AuditForge database.
//!
//! ```text
//! cargo run -p auditforge-core --example register_engagement -- "<path to auditforge.db>"
//! ```
//!
//! An administrative import, not part of the shipped application. It attributes the work to
//! the account already in the database and drives the same domain functions the UI does, so
//! the result - references, a confirmed severity on each finding, the audit trail, and an
//! immutable finalized version - is indistinguishable from a report authored in-app.
//!
//! Two engagements are transcribed from the author's own engagement reports: UbuntuServ (an
//! external-style internal test) and WinServ (an authenticated assumed-breach assessment).
//! Every answer is `user_provided`: nothing here is model-generated, and no facts, CVSS
//! scores or identifiers absent from the sources have been added.

use auditforge_core::database::Database;
use auditforge_core::models::*;
use auditforge_core::{engagements, findings, reports, templates};

fn main() -> auditforge_core::Result<()> {
    let db_path = std::env::args()
        .nth(1)
        .expect("usage: register_engagement <path to auditforge.db>");
    let db = Database::open(std::path::Path::new(&db_path))?;
    db.with_conn(templates::sync_builtins)?;

    let actor = db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, username FROM users ORDER BY created_at LIMIT 1",
            [],
            |row| {
                Ok(Actor {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    session_id: String::new(),
                })
            },
        )
        .map_err(|_| auditforge_core::Error::validation("no account exists in that database"))
    })?;
    println!("acting as: {}\n", actor.username);

    register_ubuntu(&db, &actor)?;
    register_winserv(&db, &actor)?;
    Ok(())
}

/// A guided answer, always attributed to the author.
fn a(section: &str, question: &str, value: &str) -> AnswerInput {
    AnswerInput {
        section_key: section.into(),
        question_key: question.into(),
        value: value.into(),
        provenance: Provenance::UserProvided,
        ai_request_id: None,
    }
}

fn finalize(
    db: &Database,
    actor: &Actor,
    report: &Report,
    engagement_ref: &str,
    finding_ids: Vec<String>,
    answers: Vec<AnswerInput>,
    notes: &str,
) -> auditforge_core::Result<()> {
    reports::save_draft(
        db,
        actor,
        &report.id,
        &ReportDraftUpdate {
            answers,
            finding_ids: Some(finding_ids),
            ..Default::default()
        },
    )?;

    let readiness = db.with_conn(|conn| reports::readiness(conn, &report.id))?;
    for issue in readiness
        .issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Blocker)
    {
        println!("  BLOCKER: {}", issue.message);
    }
    if !readiness.can_finalize {
        return Err(auditforge_core::Error::validation(
            "report is not ready to finalize",
        ));
    }
    let sealed = reports::finalize(db, actor, &report.id, true, notes)?;
    println!(
        "  finalized {} as {} ({})\n",
        report.report_ref, sealed.version_label, engagement_ref
    );
    Ok(())
}

fn confirm(
    db: &Database,
    actor: &Actor,
    finding_id: &str,
    severity: Severity,
    rationale: &str,
) -> auditforge_core::Result<()> {
    findings::set_severity(
        db,
        actor,
        finding_id,
        &SeverityDecision {
            severity,
            confirmed_by_user: true,
            rationale: rationale.into(),
        },
    )?;
    Ok(())
}

// ==================================================================== UbuntuServ

fn register_ubuntu(db: &Database, actor: &Actor) -> auditforge_core::Result<()> {
    let engagement = engagements::create(
        db,
        actor,
        &NewEngagement {
            name: "LabNet Internal Assessment - UbuntuServ".into(),
            client_name: "LabNet (internal home lab)".into(),
            start_date: Some("2026-08-17".into()),
            end_date: Some("2026-08-17".into()),
            objectives: "Determine whether UbuntuServ can be compromised from an attacker \
                         already positioned on the internal network, following a repeatable \
                         methodology: scope & recon, scan & enumerate, exploit / validate, \
                         remediate, verify, then clean up and report."
                .into(),
            scope: "Isolated LabNet 10.10.10.0/24 only. Authorized lab; no external systems. \
                    Target: UbuntuServ 10.10.10.3 (Ubuntu 24.04.4 LTS). Attacker: Kali \
                    10.10.10.4."
                .into(),
            out_of_scope: "Any system outside LabNet 10.10.10.0/24, and all external or \
                           internet-facing systems."
                .into(),
            authorization_ref: "Authorized home-lab engagement (isolated LabNet network)".into(),
            rules_of_engagement: "Testing confined to LabNet 10.10.10.0/24. The credential test \
                                  used a deliberately weak throwaway account (testuser), created \
                                  for the test and removed on completion."
                .into(),
            notes: "VirtualBox VMs on the D: SSD, shared NAT Network \"LabNet\". Companion to \
                    the VM Lab Playbook."
                .into(),
            team: vec![NewTeamMember {
                name: "Spencer".into(),
                role: "Lead tester (Kali attacker box, 10.10.10.4)".into(),
                email: String::new(),
            }],
        },
    )?;
    println!("UbuntuServ engagement: {}", engagement.engagement_ref);

    let f01 = findings::create(
        db,
        actor,
        &NewFinding {
            title: "SSH Weak Password (Brute-Forceable)".into(),
            engagement_id: Some(engagement.id.clone()),
            category: "Credentials".into(),
            affected_asset: "UbuntuServ 10.10.10.3 - SSH / OpenSSH 9.6p1 (TCP 22)".into(),
            description: "A user account (testuser) used a weak dictionary password, allowing a \
                          credential brute-force attack over SSH and full interactive shell \
                          access as that user."
                .into(),
            technical_details: "hydra -l testuser -P /usr/share/wordlists/rockyou.txt \
                                ssh://10.10.10.3 -t 4\n[22][ssh] host: 10.10.10.3  login: \
                                testuser  password: dragon\n1 valid password found (~75 tries, \
                                ~1 minute)."
                .into(),
            impact: "A foothold suitable for privilege escalation and lateral movement, with a \
                     full interactive shell as the user."
                .into(),
            likelihood: "High - the password fell in about 75 tries (roughly a minute) using a \
                         standard wordlist."
                .into(),
            risk: "High - trivially exploitable from an on-network position, yielding direct \
                   shell access."
                .into(),
            root_cause: "A weak, dictionary-based password was permitted on an interactive \
                         account, with password authentication enabled and no brute-force \
                         protection."
                .into(),
            remediation: "Installed fail2ban to ban brute-force sources; moved to key-based \
                          authentication (ssh-copy-id) and set PasswordAuthentication no in \
                          sshd_config; restarted ssh."
                .into(),
            recommendation: "Enforce strong-password or key-based authentication across all \
                             hosts, and deploy brute-force protection (fail2ban) as defense in \
                             depth."
                .into(),
            ..Default::default()
        },
    )?;
    confirm(
        db,
        actor,
        &f01.id,
        Severity::High,
        "Brute-forced to a full shell in about a minute",
    )?;
    findings::update(
        db,
        actor,
        &f01.id,
        &FindingUpdate {
            verification_status: Some(VerificationStatus::Remediated),
            reference_links: Some("MITRE ATT&CK: Initial Access, Credential Access".into()),
            ..Default::default()
        },
    )?;
    println!("  finding: {} (High)", f01.finding_ref);

    let f02 = findings::create(
        db,
        actor,
        &NewFinding {
            title: "Anonymous SMB Enumeration".into(),
            engagement_id: Some(engagement.id.clone()),
            category: "Configuration".into(),
            affected_asset: "UbuntuServ 10.10.10.3 - SMB / Samba 4 (TCP 139, 445)".into(),
            description: "Null sessions let an unauthenticated attacker enumerate share names \
                          and host/OS information. The share contents themselves still required \
                          authentication."
                .into(),
            technical_details: "smbclient -L //10.10.10.3 -N  ->  shares: public, share, IPC$\n\
                                enum4linux -a 10.10.10.3  ->  [+] Server allows sessions using \
                                username '', password ''\nsmbclient //10.10.10.3/public -N  ->  \
                                Anonymous login successful; tree connect failed: \
                                NT_STATUS_ACCESS_DENIED."
                .into(),
            impact: "Information disclosure that lowers attacker effort during reconnaissance. \
                     Share data itself was not exposed."
                .into(),
            likelihood: "Medium - trivially performed by any unauthenticated host on the \
                         segment."
                .into(),
            risk: "Medium - enumeration only; no direct data exposure.".into(),
            root_cause: "Samba was configured to permit anonymous / null sessions and guest \
                         mapping."
                .into(),
            remediation: "Set map to guest = never, restrict anonymous = 2 and server min \
                          protocol = SMB2 in smb.conf [global]; restarted smbd."
                .into(),
            recommendation: "Disable anonymous access on all services and require \
                             authentication before any enumeration is possible."
                .into(),
            ..Default::default()
        },
    )?;
    confirm(
        db,
        actor,
        &f02.id,
        Severity::Medium,
        "Enumeration only; share data required auth",
    )?;
    findings::update(
        db,
        actor,
        &f02.id,
        &FindingUpdate {
            verification_status: Some(VerificationStatus::Remediated),
            reference_links: Some("MITRE ATT&CK: Discovery".into()),
            ..Default::default()
        },
    )?;
    println!("  finding: {} (Medium)", f02.finding_ref);

    let report = reports::create(
        db,
        actor,
        &NewReport {
            title: "LabNet Internal Assessment - UbuntuServ".into(),
            template_key: "engagement_report".into(),
            engagement_id: Some(engagement.id.clone()),
            classification: Some(Classification::Internal),
        },
    )?;
    println!("  report: {}", report.report_ref);

    let answers =
        vec![
        a("executive_summary", "exec_purpose",
          "An authorized internal assessment of UbuntuServ (10.10.10.3) on the isolated LabNet \
           segment, to determine whether the host could be compromised from an attacker already \
           positioned on the internal network."),
        a("executive_summary", "exec_outcome",
          "UbuntuServ exposed four services (SSH, HTTP, and SMB on 139/445). The service \
           software was fully patched, so the exploitable risk lived in configuration \
           (anonymous SMB enumeration) and credentials (a weak SSH password). Both were \
           demonstrated, remediated, and verified. Findings: 1 High, 1 Medium."),
        a("executive_summary", "exec_key_risks",
          "F-01 (High): a weak SSH password was brute-forced to a full shell in about a minute. \
           F-02 (Medium): anonymous SMB null sessions disclosed share names and host/OS \
           information."),
        a("executive_summary", "exec_next_steps",
          "Both findings are remediated and verified. Extend the same controls - key-based SSH, \
           brute-force protection, and no anonymous SMB - across all hosts, and segment the \
           network so a single foothold cannot reach every system."),
        a("scope", "scope_in",
          "UbuntuServ - 10.10.10.3 (Ubuntu 24.04.4 LTS), across all listening services. \
           Attacker position: Kali 10.10.10.4 on the same segment."),
        a("scope", "scope_out",
          "Any system outside LabNet 10.10.10.0/24, and all external or internet-facing \
           systems."),
        a("scope", "scope_limitations",
          "Testing was confined to the isolated LabNet 10.10.10.0/24. The credential finding \
           used a deliberately weak throwaway account (testuser), created for the test and \
           removed on completion."),
        a("objectives", "objectives_primary",
          "Determine whether UbuntuServ can be compromised from an attacker already on the \
           internal network, following a repeatable methodology: scope & recon, scan & \
           enumerate, exploit / validate, remediate, verify, then clean up and report."),
        a("objectives", "objectives_success",
          "A weakness counts as demonstrated only when it is safely proven to have impact and \
           captured as evidence, and counts as closed only when the exact attack is re-run and \
           confirmed to fail."),
        a("activities", "activities_performed",
          "Step 1 - Identify the target: confirmed reachability of 10.10.10.3 and mapped live \
           hosts with nmap -sn 10.10.10.0/24 (target 10.10.10.3, attacker 10.10.10.4).\n\n\
           Step 2 - Scan services: nmap -sV 10.10.10.3 found 22/tcp OpenSSH 9.6p1, 80/tcp nginx \
           1.24.0, and 139/445 Samba 4.\n\n\
           Step 3 - Check known exploits: searchsploit returned no results for OpenSSH 9.6 or \
           nginx 1.24 - the software was patched, so the work pivoted to configuration and \
           credentials.\n\n\
           Step 4 - Enumerate SMB: smbclient -L -N and enum4linux showed the server accepted \
           null sessions and exposed share names (public, share, IPC$).\n\n\
           Step 5 - Attempt anonymous read: smbclient //10.10.10.3/public -N logged in \
           anonymously but the tree connect was denied - only enumeration was exposed, not the \
           data.\n\n\
           Step 6 - Brute-force SSH: hydra against the weak testuser account recovered the \
           password in about a minute, yielding a full shell.\n\n\
           Step 7 - Remediate: hardened Samba (map to guest = never, restrict anonymous = 2, \
           SMB2 minimum) and SSH (fail2ban, key-based auth, PasswordAuthentication no).\n\n\
           Step 8 - Verify: SMB enumeration was denied, fail2ban banned the attacker IP, and \
           the SSH brute force found nothing.\n\n\
           Step 9 - Clean up: removed the throwaway testuser account and restored the \
           environment."),
        a("activities", "activities_participants",
          "Spencer - lead tester, operating the Kali attacker box (10.10.10.4). Target host: \
           UbuntuServ (10.10.10.3)."),
        a("outcomes", "outcomes_delivered",
          "Two findings identified, demonstrated, remediated and verified within the \
           engagement. The deliberately weak test account was removed and the environment \
           restored."),
        a("outcomes", "outcomes_open",
          "None outstanding for UbuntuServ. The broader recommendations - network segmentation \
           and fleet-wide credential and SMB hardening - remain for the wider environment."),
        a("recommendations", "recommendations_strategic",
          "Segment the network into VLANs with firewall / ACL rules between them, so a single \
           foothold cannot reach every host. Layer with phishing training, MFA on all remote \
           access, not exposing services to the internet, patching, least privilege, and \
           monitoring / EDR. (MITRE ATT&CK: Initial Access -> Discovery -> Lateral Movement.)"),
        a("recommendations", "recommendations_quick_wins",
          "Disable anonymous access on all services; enforce key-based SSH with \
           PasswordAuthentication no; deploy fail2ban."),
        a("timeline", "timeline_events",
          "2026-08-17 - single-session engagement: identify the target, scan services, check \
           known exploits, enumerate SMB, attempt anonymous read, brute-force SSH, remediate, \
           verify, and clean up."),
        a("conclusion", "conclusion_summary",
          "Patched software resisted known exploits; the exploitable risk lived in \
           configuration and credentials, both remediated and verified within the engagement, \
           with the test account removed afterward. Recommendations: disable anonymous access \
           on all services, enforce strong-password / key-based authentication, apply defense \
           in depth (fail2ban + keys), keep systems patched, and segment the network so a \
           single foothold cannot reach every host."),
        a("appendix", "appendix_notes",
          "Initial-access context: this engagement began with the attacker already on LabNet. \
           Gaining that position - phishing, an exposed internet-facing service, stolen, weak \
           or reused credentials, a malicious download or supply-chain compromise, or Wi-Fi / \
           physical access - is a separate, harder phase; everything in this report is what \
           happens after a foothold exists. The primary defense is network segmentation. \
           Companion to the VM Lab Playbook."),
    ];

    finalize(
        db,
        actor,
        &report,
        &engagement.engagement_ref,
        vec![f01.id, f02.id],
        answers,
        "Imported from the original engagement report; findings remediated and verified.",
    )
}

// ==================================================================== WinServ

fn register_winserv(db: &Database, actor: &Actor) -> auditforge_core::Result<()> {
    let engagement = engagements::create(
        db,
        actor,
        &NewEngagement {
            name: "LabNet Internal Assessment - WinServ (File Server)".into(),
            client_name: "LabNet (internal home lab)".into(),
            start_date: Some("2026-08-17".into()),
            end_date: Some("2026-08-17".into()),
            objectives: "Evaluate WinServ's configuration, access controls and blast radius \
                         from an assumed-breach position, using valid credentials the tester \
                         already held."
                .into(),
            scope: "Isolated LabNet 10.10.10.0/24 only. Authorized lab; no external systems. \
                    Target: WinServ 10.10.10.5 (Windows Server 2025, Build 26100, standalone \
                    WORKGROUP). Attacker: Kali 10.10.10.4."
                .into(),
            out_of_scope: "Any system outside LabNet 10.10.10.0/24, and all external or \
                           internet-facing systems."
                .into(),
            authorization_ref: "Authorized home-lab engagement (isolated LabNet network)".into(),
            rules_of_engagement: "Authenticated / assumed-breach assessment performed with \
                                  valid credentials the tester already held. Authentication was \
                                  not defeated; no credential was cracked or bypassed."
                .into(),
            notes: "VirtualBox VMs on the D: SSD, shared NAT Network \"LabNet\". Second in the \
                    LabNet engagement series (companion to the UbuntuServ engagement)."
                .into(),
            team: vec![NewTeamMember {
                name: "Spencer".into(),
                role: "Lead tester (Kali attacker box, 10.10.10.4)".into(),
                email: String::new(),
            }],
        },
    )?;
    println!("WinServ engagement: {}", engagement.engagement_ref);

    let f01 = findings::create(
        db,
        actor,
        &NewFinding {
            title: "Exposed WinRM + Admin Credential = Full Remote Compromise".into(),
            engagement_id: Some(engagement.id.clone()),
            category: "Access control / Remote management".into(),
            affected_asset: "WinServ 10.10.10.5 - WinRM (TCP 5985)".into(),
            description: "WinRM is reachable and a valid local Administrator credential \
                          authenticates with admin rights, granting an interactive remote shell \
                          (Evil-WinRM)."
                .into(),
            technical_details: "netexec winrm 10.10.10.5 -u Administrator -p '<redacted>'  ->  \
                                (Pwn3d!)\nevil-winrm -i 10.10.10.5 -u Administrator  ->  whoami: \
                                winserv\\administrator; Get-ChildItem D:\\Departments\\FINANCE \
                                readable (RBAC bypassed)."
                .into(),
            impact: "Complete control of the server. Admin overrides NTFS and reads/writes every \
                     department share regardless of the RBAC, and can create accounts, dump \
                     credentials, and persist."
                .into(),
            likelihood: "An exposed management service combined with a valid administrator \
                         credential; realistic wherever admin credentials are reused or \
                         harvested."
                .into(),
            risk: "Critical - full remote compromise that bypasses all file-share RBAC.".into(),
            root_cause: "WinRM is exposed to the general network segment, and the local \
                         Administrator credential is usable remotely."
                .into(),
            remediation: "Restrict WinRM with a firewall rule to a management subnet (or disable \
                          it if unused); protect the admin credential (long, unique, rotated - \
                          consider LAPS for local admin); administer from a dedicated admin \
                          host; enable logging and monitoring."
                .into(),
            recommendation: "Administer from a dedicated management host on a restricted subnet, \
                             and vault and rotate local administrator credentials."
                .into(),
            ..Default::default()
        },
    )?;
    confirm(
        db,
        actor,
        &f01.id,
        Severity::Critical,
        "Valid admin over exposed WinRM yields full remote control, overriding all RBAC",
    )?;
    findings::update(
        db,
        actor,
        &f01.id,
        &FindingUpdate {
            reference_links: Some("MITRE ATT&CK: Lateral Movement, Execution".into()),
            ..Default::default()
        },
    )?;
    println!("  finding: {} (Critical)", f01.finding_ref);

    let f02 = findings::create(
        db,
        actor,
        &NewFinding {
            title: "C:\\Users Over-Shared to Authenticated Users".into(),
            engagement_id: Some(engagement.id.clone()),
            category: "Configuration / Information disclosure".into(),
            affected_asset: "WinServ 10.10.10.5 - SMB share \"Users\" (TCP 445)".into(),
            description: "A share named \"Users\" maps to C:\\Users and is readable by any \
                          authenticated user (confirmed as HR01)."
                .into(),
            technical_details: "smbclient //10.10.10.5/Users -U HR01  ->  lists Default, HR01, \
                                Public, desktop.ini (other profiles' contents still protected by \
                                NTFS)."
                .into(),
            impact: "Information disclosure of the user/profile layout and unnecessary attack \
                     surface. NTFS still protects the contents of individual profiles."
                .into(),
            likelihood: "Any authenticated user can connect to the share.".into(),
            risk: "Medium - enumeration of the profile layout; no protected data exposed.".into(),
            root_cause: "The Users share (and an empty Shares share) are exposed to all \
                         authenticated users rather than restricted to administrators."
                .into(),
            remediation: "Remove the Users share (and the empty Shares share) or restrict them \
                          to administrators."
                .into(),
            recommendation: "Expose only the shares users need, and restrict administrative or \
                             system paths to administrators."
                .into(),
            ..Default::default()
        },
    )?;
    confirm(
        db,
        actor,
        &f02.id,
        Severity::Medium,
        "Profile-layout disclosure; NTFS still protects contents",
    )?;
    println!("  finding: {} (Medium)", f02.finding_ref);

    let f03 = findings::create(
        db,
        actor,
        &NewFinding {
            title: "SMB Signing Not Required".into(),
            engagement_id: Some(engagement.id.clone()),
            category: "Configuration".into(),
            affected_asset: "WinServ 10.10.10.5 - SMB (TCP 445), signing:False".into(),
            description: "The server does not require SMB signing.".into(),
            technical_details: "netexec smb 10.10.10.5  ->  signing:False (SMBv1 disabled).".into(),
            impact: "Enables NTLM/SMB relay attacks - a coerced authentication can be relayed to \
                     this host."
                .into(),
            likelihood: "Requires a coerced or relayed authentication on the segment.".into(),
            risk: "Medium - relay exposure rather than direct compromise.".into(),
            root_cause: "SMB signing is not enforced in policy.".into(),
            remediation: "Enforce \"Microsoft network server: Digitally sign communications \
                          (always)\" via Group Policy / registry, and restrict NTLM where \
                          possible."
                .into(),
            recommendation: "Require SMB signing across the fleet and reduce NTLM usage.".into(),
            ..Default::default()
        },
    )?;
    confirm(
        db,
        actor,
        &f03.id,
        Severity::Medium,
        "Signing not required, enabling NTLM/SMB relay",
    )?;
    println!("  finding: {} (Medium)", f03.finding_ref);

    let report = reports::create(
        db,
        actor,
        &NewReport {
            title: "Authenticated Security Assessment - WinServ".into(),
            template_key: "security_assessment".into(),
            engagement_id: Some(engagement.id.clone()),
            classification: Some(Classification::Internal),
        },
    )?;
    println!("  report: {}", report.report_ref);

    let answers = vec![
        a("executive_summary", "exec_purpose",
          "An authorized, authenticated (assumed-breach) security assessment of WinServ \
           (10.10.10.5), a standalone Windows Server 2025 file server on the isolated LabNet \
           segment, to evaluate its configuration, access controls and blast radius using valid \
           credentials the tester already held."),
        a("executive_summary", "exec_outcome",
          "WinServ exposed IIS, RPC, SMB, LDAP (AD LDS) and WinRM. The department file-share \
           RBAC (HR$/IT$/FIN$) was tested and enforces correctly. However, WinRM was exposed and \
           a valid administrator credential granted full remote control of the server, bypassing \
           all RBAC - a Critical finding. Secondary issues: C:\\Users is over-shared to \
           authenticated users, and SMB signing is not required. Findings: 1 Critical, 2 \
           Medium, plus 1 confirmed strength."),
        a("executive_summary", "exec_key_risks",
          "F-01 (Critical): exposed WinRM plus a valid admin credential yields full remote \
           control, overriding NTFS and every RBAC boundary. F-02 (Medium): the Users share \
           exposes the C:\\Users profile layout to any authenticated user. F-03 (Medium): SMB \
           signing is not required, enabling NTLM/SMB relay."),
        a("executive_summary", "exec_next_steps",
          "Remediate F-01 first - restrict WinRM to a management subnet and protect the admin \
           credential - then address F-02 and F-03. Segment the network so a workstation \
           foothold cannot reach the server."),
        a("scope", "scope_in",
          "WinServ - 10.10.10.5 (Windows Server 2025, Build 26100, standalone WORKGROUP), across \
           its exposed services. Attacker position: Kali 10.10.10.4."),
        a("scope", "scope_out",
          "Any system outside LabNet 10.10.10.0/24, and all external or internet-facing \
           systems."),
        a("scope", "scope_limitations",
          "Authenticated / assumed-breach assessment: performed with valid credentials the \
           tester already held, to evaluate configuration and blast radius. Authentication was \
           not defeated and no credential was cracked or bypassed."),
        a("objectives", "objectives_primary",
          "Evaluate WinServ's configuration, access controls and blast radius from an \
           assumed-breach position, and determine whether the department file-share RBAC holds \
           under both normal-user and administrator access."),
        a("objectives", "objectives_success",
          "The RBAC is sound only if each department can write only its own share and \
           cross-department and admin-share access is denied; the assessment records both the \
           control that holds and the exposure that defeats it."),
        a("methodology", "methodology_approach",
          "Followed the repeatable process: scope & recon, scan & enumerate, authenticated \
           access & RBAC validation, impact demonstration, then remediation guidance. Each step \
           was tagged as an authenticated assessment action or an unauthenticated attack; the \
           engagement was almost entirely assessment - no credential was cracked or bypassed."),
        a("methodology", "methodology_standards",
          "Least-privilege / RBAC validation, using the mirror-image test (HR01 vs FIN01) to \
           prove cross-department denial. MITRE ATT&CK framing for the initial-access context."),
        a("methodology", "methodology_tools", "nmap, netexec, smbclient, enum4linux, evil-winrm."),
        a("environment", "environment_description",
          "Standalone Windows Server 2025 (Build 26100) in a WORKGROUP, acting as a departmental \
           file server on the isolated LabNet 10.10.10.0/24 (VirtualBox NAT network). RDP (3389) \
           is not exposed; the host firewall filters all but the listed services."),
        a("environment", "environment_assets",
          "Exposed services: IIS, RPC, SMB (445), LDAP (AD LDS) and WinRM (5985). Department \
           shares HR$, IT$, FIN$ with per-department RBAC, plus additional Users (maps to \
           C:\\Users) and Shares."),
        a("risk_analysis", "risk_overall",
          "The file-server RBAC design is sound and was verified - each department writes only \
           its own share, and cross-department and admin-share access is denied (a confirmed \
           strength). That control protects against a normal-user compromise, but it is entirely \
           defeated by the Critical WinRM/admin exposure: an administrator overrides NTFS and \
           reads every share regardless of the RBAC."),
        a("risk_analysis", "risk_business",
          "A single reused or harvested administrator credential, used over the exposed WinRM \
           service, results in total compromise of the file server and all department data - the \
           RBAC does not contain an admin-level breach."),
        a("risk_analysis", "risk_assumptions",
          "Assumes the attacker is already on the internal segment (the assumed-breach premise) \
           and can obtain or reuse a valid administrator credential; reaching the LAN in the \
           first place is a separate initial-access phase."),
        a("remediation", "remediation_priorities",
          "1) F-01: restrict WinRM to a management subnet (or disable if unused) and protect the \
           local admin credential (long, unique, rotated; consider LAPS); administer from a \
           dedicated admin host. 2) F-02: remove or restrict the Users and Shares shares to \
           administrators. 3) F-03: require SMB signing via Group Policy and restrict NTLM where \
           possible."),
        a("remediation", "remediation_effort",
          "F-01 is a firewall rule plus credential hygiene; F-02 is a share change; F-03 is a \
           Group Policy / registry setting. All are low-effort configuration changes."),
        a("remediation", "remediation_verification",
          "F-01: WinRM unreachable from a normal segment and admin logons alerted. F-02: a \
           normal user can no longer connect to \\\\WINSERV\\Users. F-03: netexec reports \
           signing:True."),
        a("recommendations", "recommendations_strategic",
          "Segment the network so a workstation foothold cannot reach the server; enforce MFA on \
           all remote access; monitor administrative logons; vault and rotate local admin \
           credentials. (MITRE ATT&CK: Initial Access -> Discovery -> Lateral Movement.)"),
        a("recommendations", "recommendations_quick_wins",
          "Restrict or disable WinRM; remove the over-shared Users/Shares shares; require SMB \
           signing."),
        a("conclusion", "conclusion_summary",
          "The file-server RBAC is well designed and verified, but it is defeated by the \
           Critical WinRM/admin exposure (F-01) and weakened by over-sharing (F-02) and missing \
           SMB signing (F-03). Priority: remediate F-01 first (restrict WinRM and protect admin \
           credentials), then F-02 and F-03. Broader: segment the network so a foothold cannot \
           reach the server, enforce MFA on remote access, and monitor administrative logons."),
        a("appendix", "appendix_notes",
          "Second in the LabNet engagement series (companion to the UbuntuServ engagement \
           report). Initial-access context: the engagement began with the attacker already on \
           LabNet; gaining that position is a separate, harder phase. A normal-user compromise \
           is contained by the RBAC, but a compromised administrator credential over exposed \
           WinRM is total. Primary defense: network segmentation."),
    ];

    finalize(
        db,
        actor,
        &report,
        &engagement.engagement_ref,
        vec![f01.id, f02.id, f03.id],
        answers,
        "Imported from the original WinServ assessment; F-01 Critical, remediation recommended.",
    )
}
