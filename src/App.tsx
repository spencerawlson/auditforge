/**
 * Routing and the authentication gate.
 *
 * Every route below `RequireSession` is unreachable without a live session; the backend
 * enforces the same rule independently, so this is convenience rather than security.
 */

import { HashRouter, Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/AppShell";
import { LoadingBlock } from "@/components/ui";
import { ToastProvider } from "@/components/Toast";
import { AuthScreen } from "@/features/auth/AuthScreen";
import { SessionProvider, useSession } from "@/features/auth/SessionProvider";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { EngagementDetailPage } from "@/features/engagements/EngagementDetailPage";
import { EngagementsPage } from "@/features/engagements/EngagementsPage";
import { EvidencePage } from "@/features/evidence/EvidencePage";
import { FindingDetailPage } from "@/features/findings/FindingDetailPage";
import { FindingsPage } from "@/features/findings/FindingsPage";
import { IncidentDetailPage } from "@/features/incidents/IncidentDetailPage";
import { IncidentsPage } from "@/features/incidents/IncidentsPage";
import { PostmortemDetailPage } from "@/features/postmortems/PostmortemDetailPage";
import { PostmortemsPage } from "@/features/postmortems/PostmortemsPage";
import { ReportEditorPage } from "@/features/reports/ReportEditorPage";
import { ReportsPage } from "@/features/reports/ReportsPage";
import { TemplatesPage } from "@/features/reports/TemplatesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";

export function App() {
  return (
    <ToastProvider>
      <SessionProvider>
        <HashRouter>
          <RequireSession>
            <AppShell>
              <Routes>
                <Route path="/" element={<DashboardPage />} />
                <Route path="/engagements" element={<EngagementsPage />} />
                <Route path="/engagements/:id" element={<EngagementDetailPage />} />
                <Route path="/reports" element={<ReportsPage />} />
                <Route path="/reports/:id" element={<ReportEditorPage />} />
                <Route path="/findings" element={<FindingsPage />} />
                <Route path="/findings/:id" element={<FindingDetailPage />} />
                <Route path="/evidence" element={<EvidencePage />} />
                <Route path="/incidents" element={<IncidentsPage />} />
                <Route path="/incidents/:id" element={<IncidentDetailPage />} />
                <Route path="/postmortems" element={<PostmortemsPage />} />
                <Route path="/postmortems/:id" element={<PostmortemDetailPage />} />
                <Route path="/templates" element={<TemplatesPage />} />
                <Route path="/settings" element={<SettingsPage />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Routes>
            </AppShell>
          </RequireSession>
        </HashRouter>
      </SessionProvider>
    </ToastProvider>
  );
}

function RequireSession({ children }: { children: React.ReactNode }) {
  const { status, user } = useSession();

  if (!status) {
    return (
      <div style={{ display: "grid", placeItems: "center", height: "100%" }}>
        <LoadingBlock label="Starting AuditForge" />
      </div>
    );
  }
  if (!user) return <AuthScreen />;
  return <>{children}</>;
}
