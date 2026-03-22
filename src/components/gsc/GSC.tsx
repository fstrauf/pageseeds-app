import { useState } from 'react'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { GscAuth } from './GscAuth'
import { GscDashboard } from './GscDashboard'
import { GscMovers } from './GscMovers'
import { GscIndexing } from './GscIndexing'
import { GscCoverage } from './GscCoverage'
import { GscRedirects } from './GscRedirects'
import type { Project } from '../../lib/types'

interface Props {
  projectId: string
  project?: Project
}

export function GSC({ projectId, project }: Props) {
  const [authVersion, setAuthVersion] = useState(0)

  const siteUrl = project?.site_url ?? ''

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <Tabs defaultValue="auth" className="flex flex-col h-full">
        <div
          className="px-4 pt-3 border-b shrink-0"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <TabsList className="bg-card border border-border">
            {['auth', 'dashboard', 'movers', 'indexing', 'coverage', 'redirects'].map(tab => (
              <TabsTrigger
                key={tab}
                value={tab}
                className="text-xs capitalize data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
              >
                {tab}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>

        <TabsContent value="auth" className="flex-1 overflow-y-auto mt-0 p-4">
          <GscAuth projectId={projectId} onAuthenticated={() => setAuthVersion(v => v + 1)} />
        </TabsContent>

        <TabsContent value="dashboard" className="flex-1 overflow-hidden mt-0 p-0">
          <GscDashboard siteUrl={siteUrl} authVersion={authVersion} />
        </TabsContent>

        <TabsContent value="movers" className="flex-1 overflow-hidden mt-0 p-0">
          <GscMovers siteUrl={siteUrl} authVersion={authVersion} />
        </TabsContent>

        <TabsContent value="indexing" className="flex-1 overflow-hidden mt-0 p-4">
          <GscIndexing
            projectId={projectId}
            siteUrl={siteUrl}
            authVersion={authVersion}
          />
        </TabsContent>

        <TabsContent value="coverage" className="flex-1 overflow-y-auto mt-0 p-4">
          <GscCoverage />
        </TabsContent>

        <TabsContent value="redirects" className="flex-1 overflow-y-auto mt-0 p-4">
          <GscRedirects />
        </TabsContent>
      </Tabs>
    </div>
  )
}
