import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { KeywordResearch } from './KeywordResearch'
import { BacklinkView } from './BacklinkView'
import { TrafficOverview } from './TrafficOverview'
import { KeywordCoveragePanel } from './KeywordCoverage'
import { ResearchShortlist } from './ResearchShortlist'
import type { Project, Task } from '../../lib/types'

interface Props {
  projectId: string
  project?: Project
  onRunTasks?: (tasks: Task[]) => void
}

export function SEO({ projectId, project, onRunTasks }: Props) {
  return (
    <div className="flex flex-col h-full overflow-hidden">
      <Tabs defaultValue="keywords" className="flex flex-col h-full">
        <div
          className="px-4 pt-3 border-b shrink-0"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <TabsList className="bg-card border border-border">
            {[
              { value: 'keywords', label: 'Keywords' },
              { value: 'shortlist', label: 'Shortlist' },
              { value: 'coverage', label: 'Coverage' },
              { value: 'backlinks', label: 'Backlinks' },
              { value: 'traffic', label: 'Traffic' },
            ].map(tab => (
              <TabsTrigger
                key={tab.value}
                value={tab.value}
                className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
              >
                {tab.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>

        <TabsContent value="keywords" className="flex-1 overflow-hidden mt-0 p-0">
          <KeywordResearch projectId={projectId} />
        </TabsContent>

        <TabsContent value="shortlist" className="flex-1 overflow-hidden mt-0 p-0">
          <ResearchShortlist projectId={projectId} />
        </TabsContent>

        <TabsContent value="coverage" className="flex-1 overflow-hidden mt-0 p-0">
          <KeywordCoveragePanel project={project ?? null} onRunTasks={onRunTasks} />
        </TabsContent>

        <TabsContent value="backlinks" className="flex-1 overflow-hidden mt-0 p-0">
          <BacklinkView projectId={projectId} />
        </TabsContent>

        <TabsContent value="traffic" className="flex-1 overflow-hidden mt-0 p-0">
          <TrafficOverview projectId={projectId} />
        </TabsContent>
      </Tabs>
    </div>
  )}
