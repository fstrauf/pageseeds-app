import { useState } from 'react'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { ClaritySettings } from './ClaritySettings'
import { ClaritySummary } from './ClaritySummary'
import type { Project } from '../../lib/types'

interface Props {
  projectId: string
  project?: Project
}

export function Clarity({ projectId, project }: Props) {
  const [version, setVersion] = useState(0)

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <Tabs defaultValue="summary" className="flex flex-col h-full">
        <div
          className="px-4 pt-3 border-b shrink-0"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <TabsList className="bg-card border border-border">
            {['summary', 'settings'].map(tab => (
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

        <TabsContent value="summary" className="flex-1 overflow-y-auto mt-0 p-4">
          <ClaritySummary project={project} version={version} />
        </TabsContent>

        <TabsContent value="settings" className="flex-1 overflow-y-auto mt-0 p-4">
          <ClaritySettings
            projectId={projectId}
            project={project}
            onChanged={() => setVersion(v => v + 1)}
          />
        </TabsContent>
      </Tabs>
    </div>
  )
}
